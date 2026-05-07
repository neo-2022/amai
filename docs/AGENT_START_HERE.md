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

Если нужен не только status/startup вход, но и целостная картина того, что вообще заложено в
`Amai`, отдельно открыть `AMAI_SYSTEM_OVERVIEW.md`.

Этот обзорный документ нужен затем, чтобы новый агент не сужал проект до одной из частей:
- не сводил `Amai` только к continuity;
- не сводил `Amai` только к retrieval;
- не сводил `Amai` только к task-memory;
- не забывал про интеграции, платформенную нейтральность, host/OS слой и научный advisory contour.

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
- `AMAI_SYSTEM_OVERVIEW.md`
- `AGENT_START_HERE.md`
- `IMPLEMENTATION_STATUS.md`

Смысл корня:
- понять, что это за проект;
- понять общую концептуальную картину;
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
- canonical implementation ladder и запрет dashboard-first drift держатся не здесь, а в
  `IMPLEMENTATION_STATUS.md`; если работа началась с витрины раньше foundation/workline слоя, это
  считается ошибкой, а не допустимым ускорением.

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
- не терял рабочую линию между разными рабочими поверхностями и сессиями;
- не смешивал разные проекты по умолчанию;
- поднимал полезный context pack вместо лишнего шума;
- имел source-of-truth вне IDE и вне одного конкретного чата;
- мог работать в multi-agent и multi-project режиме без тихих утечек памяти.

Проще говоря:
- это не “один длинный чат”;
- это не “ещё один vector search”;
- это не “только task tree”.

Это отдельный backend/tooling contour памяти и continuity для агентов.

Каноническая framing здесь такая:
- `Amai` — независимое ядро памяти и continuity;
- чат, окно IDE, новая сессия, `.txt` файл или другой client surface — только способ получить из
  `Amai` уже восстановленное рабочее состояние;
- transcript/history чата не является source of truth для памяти проекта.

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
- `Этап 4` `workspace restore pack`, `Этап 5` `semantic + temporal strengthening`, `Этап 6` `multi-agent shared/private memory`, `Этап 7` `compare + benchmark plane`, `Этап 8` `procedural memory`, `Этап 9` `forgetting / consolidation / pruning` и `Этап 10` `governance / safety / evaluator loop` закрыты по текущему internal status checklist и fresh proof bundle.

Важная оговорка:
- `closed` здесь означает `закрыто по внутреннему status/proof контурy проекта`;
- если текущая задача требует свежий эксплуатационный verdict, агент обязан заново прогнать matching bundle из `IMPLEMENTATION_GATES.md`;
- если fresh proof, dashboard/raw lane или external benchmark maturity не подтверждены, агент обязан пометить claim как `proof-refresh-required` или `not-fully-materialized`, а не повторять старый green verdict.

### Что уже есть, но ещё не является полной целевой реализацией

Есть сильный промежуточный baseline:
- continuity между рабочими поверхностями уже работает на prompt/machine-readable стороне, но
  host-side clean-chat handoff ещё не полностью бесшовный во всех клиентах и средах;
- даже там, где automatic clean-chat bridge ещё не materialized, compact-chat уже не сводит manual fallback к codex-only подсказке: current client surface, startup path и reconnect/fresh-chat assist теперь surfaced по реальному клиенту, а dashboard KPI selector ещё и показывает текущий auto-launch status / unavailable reason / UX boundary для clean-chat path;
- pending return и resume obligations уже surfaced;
- dashboard/eval/compare contours уже есть;
- scientific reinforcement overlay уже начат как отдельная queue-driven надстройка поверх Stage 7 / 9 / 10;
- часть probabilistic/statistical surfaces уже production-visible только как bounded advisory/read-only slices, а не как truth-authoritative decision layer.
- `KAN-style context-pack utility explain` уже зафиксирован в
  `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` как `Candidate Queue 4A`, но только
  со статусом `research_candidate / spec_only / not_materialized`; это не live
  runtime contour и не core roadmap promise.

### Что ещё не materialized полностью

Пока ещё не доведены до полного target-state:
- полноценный graph-wide restore continuity beyond уже materialized `workspace_restore_pack`;
- по-настоящему бесшовное продолжение длинной работы через новые client/runtime surfaces с
  несколькими старыми линиями работы.
- external long-term memory benchmark registry из `IMPLEMENTATION_GATES.md` уже surfaced как отдельный prep/proof contour, но не как полный scored maturity verdict: LongMemEval/MemoryAgentBench/LoCoMo и AMA-Bench имеют guarded normalized cases, `external-memory-run/score` покрыты synthetic smoke, LongMemEval, AMA-Bench и bounded `MemoryAgentBench / conflict_resolution` plus `long_range_understanding` and `test_time_learning` имеют initial, dataset-specific runtime+baseline-score proof, а benchmark-grade claim всё ещё требует full dataset runtime+score proof, semantic retrieval precision и upstream parity.
- bounded `MemoryAgentBench / accurate_retrieval` теперь тоже materialized как отдельный blocked profile: bounded runtime+score slice завершается с `3/3` baseline answers, retrieval-backed predictions по всем трём bounded cases, `top_ranked_relevant_retrieval_cases=3/3`, `gold_answer_supported_retrieval_cases=3/3` и `top_ranked_relevance_and_gold_answer_supported_retrieval_cases=3/3`, а benchmark-specific shaping на bounded slice уже отсутствует (`query_override_cases=0`, `window_override_cases=0`, `answer_extraction_cases=0`, `benchmark_specific_shaping_present=false`, `generic_runtime_maturity=true`). Boundary driven explicit runtime metric flags вместо повторного вывода по question text. Profiling-guided runtime hardening additionally dropped the current bounded cold first-case `index_project_ms` to about `0.87s` and bounded `total_case_ms.avg` to about `0.91s`, but this still remains bounded-only latency evidence rather than latency-grade maturity. Semantic relevance maturity остаётся proxy/lexical-only, а upstream scorer parity всё ещё отсутствует, поэтому этот contour всё ещё нельзя silently считать fully trusted positive bounded proof.
- Queue 1 scientific benchmark honesty ещё не равна финальному automatic promotion: measured approval остаётся human-gated.
- Queue 4 regression explain surface production-visible, но measured regression quality на live sample остаётся `insufficient_sample`, пока не накоплен двусторонний sample contour.
- Queue 5 capacity forecast surface production-visible только как forecast-only/read-only contour; exact live window values из `observe snapshot` не являются долговечным implementation-status claim.
- host-side clean-chat migration всё ещё не полностью seamless: current compact-chat runtime/API truthfully distinguishes `available_not_requested`, `bridge_unavailable`, `disabled_by_policy`, `requested` and `launch_failed`; VS Code `vscode_code_chat_cli` and non-VSCode `manual_only` boundaries are command-contract evidence only, while live client-open/session UX still requires separate proof before any seamless claim.

### Главная честная оговорка

Полноценный бесшовный переход в новую рабочую поверхность пока ещё не решён до конца.

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

Сейчас основной Stage 0-10 checklist закрыт по текущему internal status snapshot и fresh proof-refresh bundle.

### Ближайший обязательный контур

Ближайшая работа теперь не новый Stage 11 и не возврат к Stage 8.

Текущий обязательный контур:
- scientific reinforcement overlay из `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md`;
- если задача касается `KAN` / Kolmogorov-Arnold / context-pack utility
  explainability, source-of-truth сейчас живёт именно в `Candidate Queue 4A`
  этого документа, а не в `IMPLEMENTATION_STATUS.md` как materialized feature;
- status-truth / proof-refresh аудит для claims, которые в документах названы `закрыто`, `green`, `materialized` или `работает`;
- устранение cross-doc drift между `IMPLEMENTATION_STATUS.md`, `AGENT_START_HERE.md`, `AMAI_GLOBAL_MEMORY_ROADMAP.md`, `IMPLEMENTATION_GATES.md`, `README.md` и machine-readable preflight state.

Что это значит practically:
- если claim уже закрыт, но fresh proof не прогнан в текущей рабочей линии, он не должен звучать как свежий green verdict;
- если claim закрыт только внутренним harness или только external prep/synthetic smoke proof без real benchmark runtime+score lane, его нельзя называть внешне benchmark-grade;
- если runtime surface возвращает `insufficient_sample`, `pending_human_review`, `blocked`, `partial` или `proof-refresh-required`, документация должна показывать именно это состояние;
- если proof реально красный, соответствующий checkbox или status claim должен быть снят до исправления и повторной проверки.

Fresh proof-refresh 2026-04-24:
- `proof_benchmark_contamination_preflight.sh` is green;
- `proof_mcp_task_matrix.sh` is green after strict-heavy contamination preflight and explicit failed-run artifact handling;
- `proof_observability.sh` is green; dashboard `MCP task matrix compare` no longer degrades to `ещё нет данных`;
- `proof_memory_external_benchmarks.sh` is green for dataset download, adapter workspace preparation, normalized case preparation and one synthetic runtime+baseline-score smoke only; it is explicitly not a full scored external benchmark verdict;
- therefore Stage 10 is again fresh-green, while historical red runs remain useful root-cause context only.

Fresh proof-refresh 2026-04-25:
- `proof_memory_external_benchmarks.sh` reran green in the same prep/synthetic-smoke boundary only.
- MemoryAgentBench prep produced multi-GiB normalized/request artifacts, so agents must not treat this proof as a cheap recurring full benchmark runtime or as external maturity.
- `proof_observability.sh` reran green for Queue 4/5 read-only dashboard/raw snapshot guardrails only. It does not prove measured regression quality or durable capacity-quality numbers.

Fresh bounded real runtime 2026-04-26:
- `proof_memory_external_real_bounded.sh` proves a limited, dataset-specific LongMemEval `longmemeval_s_cleaned` runtime+baseline-score lane with retrieval-backed relaxed query retry.
- `proof_memory_external_real_bounded_ama_bench.sh` proves a limited, dataset-specific AMA-Bench `ama_bench_manual` runtime+baseline-score lane while keeping the official scorer contract unavailable/blocker-visible.
- It does not prove full external benchmark-grade maturity or official upstream scorer parity.
- Relaxed retry proves retrieval participation only, not semantic precision.

### Что нельзя делать сейчас

Нельзя:
- продолжать писать, что ближайший этап `Этап 8` или `Этап 1`;
- считать Stage 0-10 fresh-green без matching fresh proof bundle и dashboard/raw parity;
- принимать old docs line anchors за source-of-truth;
- замалчивать remaining external benchmark maturity gap after prep/synthetic-smoke/bounded-real proof;
- выдавать advisory/projection surfaces за truth-authoritative runtime.

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
- Stage 0-10 закрыты по internal status checklist и fresh proof bundle, а fresh proof всегда берётся из matching bundle, а не из старой фразы в документации;
- главный ближайший фокус:
  - scientific reinforcement overlay;
  - status-truth / proof-refresh аудит;
  - устранение cross-doc drift;
- главный общий закон:
  - скорость, точность, качество и правдивость нельзя разменивать друг на друга;
- главный продуктовый недостающий outcome:
  - по-настоящему бесшовное продолжение работы в новом чате.
