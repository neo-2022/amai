modified_at: 2026-03-20 20:41 MSK
Ручная сверка guide/docs: 2026-03-20 20:41 MSK

# AGENTS.md для Art-memory-agent-index (Amai)

## 1. Что это

`Art-memory-agent-index (Amai)` в текущем filesystem path `/home/art/agent-memory-index` — отдельный standalone проект.

Это не код одного конкретного продукта.
Это внешний инструмент для ИИ-агентов, который:
- хранит рабочий контекст между сессиями;
- знает, где начинается и заканчивается каждый проект;
- умеет искать по коду и документам;
- умеет собирать готовый пакет контекста для следующего шага агента;
- не даёт по умолчанию смешивать разные проекты.

## 2. Обязательный старт

Этот раздел и есть канонический рабочий алгоритм любого агента.

Любой агент обязан:
- идти по этому порядку сверху вниз;
- не заменять его собственными догадками;
- сначала поднимать статус и документы проекта;
- только потом трогать код, schema, compose и runtime.

Любой агент обязан:
1. сначала прочитать этот `AGENTS.md`;
2. затем прочитать compact preflight contract `.amai/onboarding/project-agent-preflight-agent-contract.json`, если он уже materialized;
3. затем прочитать machine-readable preflight contract `.amai/onboarding/project-agent-preflight-contract.json`, если он уже materialized;
4. затем обновить machine-readable preflight snapshot:
   - `./scripts/agent_preflight.sh --json`
5. затем прочитать `README.md`;
6. затем прочитать `docs/AGENT_START_HERE.md`;
7. затем прочитать `docs/IMPLEMENTATION_STATUS.md`;
8. затем прочитать `docs/ARCHITECTURE.md`;
9. затем прочитать `docs/OPERATIONS.md`;
10. затем прочитать `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`и `docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md`
11. если работа stage-based, архитектурная, schema/policy/truth-sensitive или рефакторит critical zone:
   - прочитать `docs/MAINTAINABILITY_ENFORCEMENT.md`;
   - запустить `./scripts/maintainability_gate.sh --json`;
   - если обновляется `docs/IMPLEMENTATION_STATUS.md`, прогнать `./scripts/implementation_status_sync_guard.sh --json`;
   - перед закрытием checkbox значимого этапа прогнать `./scripts/maintainability_stage_close_guard.sh --json`;
11a. для любой содержательной правки, даже если она маленькая и локальная:
   - не игнорировать `docs/MAINTAINABILITY_ENFORCEMENT.md`;
   - как минимум держать его как binding law;
   - помнить, что отсутствие отдельного `maintainability_gate.sh` запуска для микроправки не даёт права нарушать standard.
12. если работа идёт по implementation-stage, затем прочитать `docs/IMPLEMENTATION_GATES.md`;
12a. для любого этапа перед попыткой закрыть checkbox:
   - прогнать весь уже materialized и подходящий benchmark/proof bundle этого этапа;
   - не ждать, пока пользователь отдельно напомнит про benchmark из этого bundle;
   - не ограничиваться одним blocking-proof;
   - если изменение задело соседний shared contour (`speed / accuracy / isolation / truth / dashboard / continuity`), прогнать и companion non-regression harness для него;
   - если изменение задело retrieval/vector lane, отдельно прогнать и external/Qdrant bundle из `docs/IMPLEMENTATION_GATES.md`;
   - не использовать для stage-close урезанный benchmark режим; reduced-sample run считается только smoke и не закрывает этап;
   - если benchmark contour публикует результат на dashboard, после прогона обязательно перепроверять сам dashboard snapshot/карточку;
   - если contour не surfaced в dashboard и `IMPLEMENTATION_GATES.md` требует raw-result lane, использовать raw result и не делать вид, что dashboard-check был выполнен;
   - если хотя бы один подходящий harness не прогнан, этап закрывать запрещено;
13. если работа затрагивает память задач, compare surface или новые memory-модули:
   - прочитать `docs/AMAI_TASK_TREE_PLAN.md`;
   - прочитать `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md`;
14. только после этого трогать compose/config/schema/code.

Коротко:
- `AGENTS.md`
  - обязательный runtime/startup law;
- `docs/AGENT_START_HERE.md`
  - быстрый вход в проект и decision tree агента;
- `.amai/onboarding/project-agent-preflight-contract.json`
  - machine-readable contract: какие документы обязательны, где trunk/checklist и какие законы preflight действуют;
- `.amai/onboarding/project-agent-preflight-state.json`
  - machine-readable текущий snapshot: что уже закрыто, какой этап следующий и какие harness уже готовы;
- `docs/MAINTAINABILITY_ENFORCEMENT.md`
  - project-local binding внешнего maintainability/supportability/evolvability/anti-hardcoding стандарта; этот law нельзя игнорировать даже для маленьких содержательных правок;
- `./scripts/maintainability_gate.sh --json`
  - machine-readable change-safety gate для значимых изменений;
- `./scripts/maintainability_stage_close_guard.sh --json`
  - machine-readable closure guard: без passing результата нельзя ставить checkbox значимого этапа;
- `./scripts/implementation_status_sync_guard.sh --json`
  - machine-readable sync guard: без passing результата значимое обновление `docs/IMPLEMENTATION_STATUS.md` считается недействительным;
- `.amai/onboarding/project-maintainability-gate-state.json`
  - machine-readable gate trace, который status sync guard и closure guard сверяют с текущим `HEAD`, `IMPLEMENTATION_STATUS.md` hash и worktree fingerprint;
- `docs/IMPLEMENTATION_STATUS.md`
  - что уже сделано, что нет, что делать сейчас, и ствол рабочего чеклиста;
- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
  - по какому этапу идти дальше;
- `docs/IMPLEMENTATION_GATES.md`
  - какими proof/debug/reconcile механизмами проверять текущий этап и какие готовые benchmark-harness обязательны;
- частные планы
  - только если задача касается их модуля.

Если стек уже запущен, сначала проверить:
- `scripts/status.sh`

Если стек ещё не materialized:
- самый простой путь:
  - `scripts/onboard_local.sh --client vscode`
- если `Amai` уже стоит на удалённом Linux/VPS-host:
  - `scripts/onboard_remote_client.sh --client vscode --ssh-destination user@host --remote-repo-root /srv/amai`
- инженерный ручной путь:
  - `scripts/bootstrap_stack.sh`
- симметричное отключение клиента:
  - `scripts/disconnect_local.sh --client vscode`

## 3. Главный закон

Этот проект существует для того, чтобы агенты не смешивали проекты по умолчанию.

Значит:
- новый `repo_root` считается отдельным проектом;
- пока он не зарегистрирован, поиск контекста по нему запрещён;
- cross-project reading допускается только через relation graph и policy modes.

## 4. Канонический порядок слоёв

1. `PostgreSQL` — главный источник истины: проекты, правила доступа, метаданные и точный поиск
2. `Qdrant` — поиск похожих по смыслу фрагментов
3. `S3-compatible object storage` — хранение артефактов и готовых пакетов контекста
4. `NATS Core + JetStream` — события и рабочие очереди
5. `tree-sitter` — структура кода, символы и границы сущностей
6. `SQLite` — локальный быстрый кэш агента
7. `LanceDB` — только optional local semantic edge cache

CLI/binary canonical short name:
- `amai`

Текущий parser baseline materialized напрямую через отдельные Rust grammar crates.
Реальный AST/symbol contour сейчас есть для:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Для остальных форматов любой агент обязан понимать, что сейчас допустим только lexical fallback, а не молчаливо считать coverage полным.

## 5. Что запрещено

- превращать IDE в источник истины;
- заменять lexical/symbol retrieval только embeddings-поиском;
- подмешивать чужой проект “по похожести”;
- считать `NATS` или `Qdrant` authoritative state store;
- жёстко завязывать artifact plane только на одну реализацию вместо S3 API.
- без отдельного явного разрешения пользователя добавлять новый core/runtime/schema/eval код не на Rust;
- без отдельного явного разрешения пользователя добавлять новые `python`-тесты, `pytest`-harness или `python`-debug contour для внутренних стадий реализации;
- считать существующие `python`-пути в external benchmark compatibility contour нормой для нового core-развития проекта.

## 5A. Языковой контракт проекта

Этот проект materialize-ится как `Rust-first` system.

Значит:
- новый core-runtime, schema, CLI, verifier и основной proof contour должны писаться на Rust;
- если уже есть подходящий Rust-native harness, его нужно расширять, а не обходить новым Python-слоем;
- shell-скрипты допустимы как launcher/orchestration contour;
- `python` допустим только там, где уже есть явный upstream compatibility path для внешних benchmark-ов или dataset tooling, и это не должно становиться шаблоном для нового внутреннего слоя проекта.

Коротко:
- для внутренней реализации и проверки default language = `Rust`;
- `python` не default и не запасной путь “по привычке”.

## 6. Git-дисциплина

Каждая отдельная semantic group в этом проекте должна коммититься отдельно.
Запрещено смешивать:
- docs/governance;
- compose/bootstrap;
- schema;
- Rust CLI/indexer;
- local runtime state.

Machine-readable client install targets живут в:
- `config/client_targets.toml`

Каталоги `state/**` и `tmp/**` не должны попадать в git.

## 7. Канонический runnable path

Минимальный runnable порядок для нового агента:
1. для простого локального старта:
   - `scripts/onboard_local.sh --client vscode`
   - если нужно убрать клиент обратно:
     - `scripts/disconnect_local.sh --client vscode`
2. для ручного инженерного пути:
   - `scripts/bootstrap_stack.sh`
   - `scripts/status.sh`
   - `cargo run -- compat check`
   - `cargo run -- project register ...`
   - `cargo run -- namespace ensure ...`
   - `cargo run -- index project ...`
   - `cargo run -- context pack ...`

Для жёсткого локального proof:
- `scripts/proof_local.sh`
- `scripts/proof_hardening.sh`
- `scripts/proof_performance.sh`
- `scripts/proof_cold_benchmark.sh`
- `scripts/proof_cold_benchmark_canonical.sh`
- `scripts/proof_accuracy.sh`
- `scripts/proof_load.sh`
- `scripts/proof_hostile.sh`
- `scripts/proof_observability.sh`
- `scripts/proof_onboarding.sh`
- `scripts/proof_agent_preflight.sh`
- `scripts/proof_maintainability_gate.sh`
- `scripts/proof_maintainability_stage_close_guard.sh`
- `scripts/proof_implementation_status_sync_guard.sh`
- `scripts/proof_remote_onboarding.sh`
- `scripts/proof_client_lifecycle.sh`
- `scripts/proof_remote_ssh_config.sh`
- `scripts/proof_text_compare.sh`

Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify accuracy ...`
- `cargo run -- verify load ...`
- `cargo run -- verify hostile ...`
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`

## 8. Качество, доказанный эффект и hostile mindset
1. Минималистичный и «достаточный» подход запрещён; по умолчанию выбирается максимальный релевантный объём проверки, дебаггинга и покрытия рисков в пределах текущей задачи.
2. Формулировки `минимальная проверка`, `достаточно для pass`, `базовый smoke`, `пока этого хватит` запрещены, если пользователь прямо не приказал такой режим.
3. Любое изменение считается завершённым только после доказанного эксплуатационного эффекта.
4. Доказанный эффект означает не один удачный симптом, а разносторонний дебаггинг:
   - подтверждена основная гипотеза;
   - исключены альтернативные причины;
   - проверен hostile или negative-path;
   - доказано, что дефект не просто смещён в другой слой;
   - добавлен regression guard.
5. Локальный smoke, один grep, один запуск теста или один внешне зелёный признак не считаются достаточным доказательством.
6. Проект нужно всегда рассматривать как hostile production среду: сеть, storage, зависимости, оператор, конфигурация, input, race conditions, policy bypass, supply-chain и UX могут ломать систему.
7. Требуемый стандарт: следующий сильный ИИ инженер должен по коду и документации понимать инварианты, запускать проверки, вносить изменение, делать `rollback` и восстанавливать систему без устных пояснений.
8. Для refactor без изменения поведения требуется контрактная эквивалентность, отсутствие регресса по тестам и сохранение инвариантов.
## 9. Многоуровневый спуск к корню проблемы
1. Нельзя ограничиваться исправлением симптома на уровне UI, workflow, теста, CI, runtime, контракта, storage, transport, agent/core или документации.
2. Перед устранением дефекта нужно спускаться на уровень ниже и делать полный аудит связанного основания.
3. Если причина найдена ещё ниже, спуск продолжается до корня.
4. Проблема считается решённой только когда:
   - симптом локализован;
   - нижние слои проверены;
   - корневая причина устранена;
   - обратная связь вверх по слоям перепроверена.
## 9A. Явное переключение пользовательской рабочей линии
1. Если пользователь явно переключил задачу, фокус или active file/selected fragment, это считается новой active workline немедленно.
2. В такой ситуации агенту запрещено продолжать предыдущую active line "по инерции", даже если continuity startup или live handoff поднимали другой headline секунду назад.
3. Перед следующим содержательным ответом агент обязан materialize-ить переключение как новый `continuity_handoff`:
   - новая user-request line становится `active`;
   - предыдущая line уходит в `pending_return_queue` с причиной `interrupted_by_new_handoff`, если она не закрыта явно.
4. Если новый запрос пользователя является audit/check вопросом по другому контуру, нельзя подменять его ответом по прошлой линии только потому, что старая линия была "более активной" в continuity.
5. Если новый запрос пользователя несёт несколько явных подпунктов, агент обязан сразу materialize-ить их как durable `required_task_set`, а не держать только один `headline`.
6. Пока `required_task_set` не пуст, агенту запрещено считать работу закрытой после частичного успеха по одной подзадаче.
7. `startup_next_action`, `required_return_task` и `required_task_set` обязательны для восстановления незавершённых линий, но не имеют права переопределять новый явный user redirect внутри текущего живого диалога.
8. Если агент не сделал такой handoff и ответил по старой линии или silently narrowed multi-task запрос до одного подпункта, это считается continuity/task-switch defect, а не допустимой интерпретацией.
## 10. Язык, область действия и запрет додумывания
1. Отвечать пользователю обязательно по-русски.
2. Все правила этого файла распространяются на код, архитектуру, документацию, checklist-и, тесты, CI/CD, релизы, безопасность, UX/UI, интеграции, аудиты, агентов и платформенные сценарии.
3. Смягчать формулировки, интерпретировать, додумывать и предполагать строго запрещено.

## 11. Параллелизм и расход лимитов
1. Если подзадачи не требуют дополнительных LLM-лимитов, агент обязан распараллеливать их максимально агрессивно, пока это не ломает корректность и не создаёт конфликтов записи.
2. К этому правилу по умолчанию относятся:
   - чистые shell-команды;
   - `psql` и другие read/write операции, где конкуренция безопасна и явно контролируется;
   - локальные скрипты;
   - non-LLM CLI-проверки;
   - независимые чтения, поиски, diff, benchmark/proof run-ы и другие локальные tool-only операции.
3. Для таких подзадач нельзя выбирать последовательное выполнение просто «по привычке», если безопасная параллелизация очевидно ускоряет работу.
4. Живые LLM-subagents и другие model-level workers не подпадают под это правило автоматически: их поднимать только когда нужен именно model-level execution или validation, а не когда ту же проверку можно закрыть локальными non-LLM средствами.
5. Если пользователь явно требует живых LLM-агентов как часть проверки, это требование имеет приоритет, и агент обязан выполнить именно этот уровень верификации, а не подменять его shell-only эквивалентом.

## 11A. Обязательный first-pass side-agent contour для Gemma
1. Локальный `Gemma` через `ollama` считается не optional toy, а стандартным bounded side-agent для дешёвого first-pass analysis.
2. Если задача относится к одному из следующих классов, агент обязан сначала поднять `Gemma`-контур, а потом уже делать собственный основной проход:
   - большой файл или монолитный модуль, где нужен domain map, symbol grouping или split-plan;
   - first-pass code review, smell scan, risk scan или поиск missing tests;
   - генерация test ideas, edge cases, negative paths или regression checklist;
   - draft structuring, когда нужно быстро сжать большой кодовой surface в рабочую карту модулей, responsibilities и migration order;
   - поиск альтернативных гипотез после уже начатого локального root-cause анализа, если нужен второй дешёвый взгляд, а не final verdict.
3. Канонические launcher-ы этого контура:
   - `scripts/gemma_code_assist.sh`
   - `scripts/gemma_monolith_split.sh`
   - `scripts/ollama_chat.sh`
4. Для split/domain задач default launcher = `scripts/gemma_monolith_split.sh`; для review/bug/plan/test-idea задач default launcher = `scripts/gemma_code_assist.sh` с подходящим `--mode`.
5. Этот contour обязан работать по project-local binding law и использовать:
   - `AGENTS.md`
   - `docs/AGENT_START_HERE.md`
   - `docs/MAINTAINABILITY_ENFORCEMENT.md`
   - `docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md`
   - `docs/IMPLEMENTATION_STATUS.md`
   - `docs/IMPLEMENTATION_GATES.md`
6. `Gemma` запрещено считать final authority для:
   - exact root cause;
   - schema / protocol / security решений;
   - stage-close verdict;
   - final архитектурного решения;
   - merge-ready patch без локальной верификации;
   - любого truth-sensitive вывода, где ошибка дороже, чем экономия лимитов.
7. Если `Gemma`-contour вызван по этому правилу, агент обязан относиться к его output как к first-pass material:
   - брать полезные domain clusters, risks, draft plans и test ideas;
   - не принимать слепо claims о полноте, coverage или correctness;
   - локально проверять symbol coverage, invariants, contracts и proof impact перед содержательным изменением.
8. Пропустить этот contour можно только если:
   - задача не относится к перечисленным классам;
   - side-agent явно недоступен технически;
   - локальный non-LLM путь уже даёт тот же результат дешевле и надёжнее.
9. Если contour был пропущен при задаче из этого списка без явной причины, это считается workflow defect, а не допустимой оптимизацией.
10. Для любой содержательной работы после собственного основного прохода агент обязан сделать ещё и final cross-check через `Gemma` до финального ответа пользователю:
   - change review;
   - risk scan;
   - obvious gap scan;
   - negative-path/test-idea pass, если это уместно для текущего slice.
11. Этот final cross-check обязателен для:
   - любой правки кода;
   - любой архитектурной, policy, truth, schema или protocol decision;
   - любого stage/proof/status verdict;
   - любого claim вида `закрыто`, `зелёно`, `доказано`, `materialized`.
12. Допустимые исключения только такие:
   - `Gemma` технически недоступен, завис, таймаутит или не возвращает usable output;
   - ответ не является содержательной работой и ограничивается тривиальным ack/status without technical claim.
13. Если final cross-check не был выполнен, агент обязан:
   - считать это workflow defect;
   - не делать вид, что требование было соблюдено;
   - явно сказать пользователю, что Gemma-pass был пропущен или сорвался.
14. Если final cross-check выполнен, в финальном ответе нужно честно отражать:
   - что Gemma была использована;
   - что именно она перепроверяла;
   - где source of truth остался за локальными proof/harness/result lanes, а не за side-agent verdict.

<!-- AMAI MANAGED STARTUP INSTRUCTIONS v1 -->
# Amai continuity startup for Codex

Этот managed block должен жить в project `AGENTS.md`, а не в global config.

Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:
1. Workspace = `/home/art/agent-memory-index`. Прочитай compact agent contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-agent-contract.json` и machine-readable startup contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-contract.json`; startup contract остаётся pinned source-of-truth. До MCP tool call проверь `startup_contract_sha256 = "d41823548cc5870c08ba285d656fb9c37f7e7db911786208670c02e0248aa29a"`, `workspace_contract_required_before_tool_call = true`, `missing_or_unreadable_fail_closed = true`, `sha256_mismatch_fail_closed = true`.
2. Затем вызови MCP tool `amai_continuity_startup` с `repo_root = "/home/art/agent-memory-index"` и `namespace = "continuity"`; `project` передавай только при exact binding по repo_root. До `continuity_startup_summary` не переходи к `amai_context_pack` и новым действиям. Если tool вернул `tool_execution_failed` и detail содержит `no continuity import found for`, немедленно сделай reconcile через `./scripts/continuity_startup.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --json` (`requires_repo_root_argument = true`, `requires_namespace_argument = true`, `json_required = true`). Если embedded MCP tool call упал на transport error с detail `Transport closed` (`transport_error_detail_case_insensitive = true`), делай тот же local CLI reconcile. Если local CLI startup succeeds, классифицируй это как `stale_embedded_mcp_session` (`local_cli_success_replaces_mcp_failure = true`, `local_cli_success_replaces_transport_failure = true`, `must_request_mcp_reconnect_after_local_success = true`, `must_continue_from_local_startup_payload = true`), продолжай от локального startup payload и требуй reconnect exact front-door-ом: `./scripts/reconnect_local.sh --client codex` или `./scripts/amai_exec.sh bootstrap reconnect --client codex --yes`. Только если и MCP, и local CLI fallback провалились, объявляй continuity реально unavailable.
3. После startup прочитай runtime artifact `.amai/continuity/project-chat-startup-state.json`: `workspace_runtime_state_artifact_version` должен быть `workspace-startup-runtime-state-v4`, его пишет `amai_continuity_startup`, он обязан нести `continuity_startup_summary`. Fallback: `./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json`.
4. В runtime artifact смотри только `startup_execution_gate`, `execctl_resume_state`, `execctl_resume_contract_summary`, `execctl_resume_obligation`, `startup_next_action`, `execctl_active_lease`. Restore бери из `required_summary_fields`, obligations из `restored_obligations`. Fail-closed, если `gate_semantics_consistent != true` (`gate_semantics_consistent_true_required = true`), `startup_execution_gate.must_follow_startup_next_action != true`, `startup_execution_gate.unrelated_work_allowed != false`, `startup_execution_gate.must_read_prompt_text_before_reply != true` или `startup_execution_gate.no_silent_drop != true`.
5. Resume law: если `startup_execution_gate.required_action_kind_when_resume_required == "resume_required_return_task"`, `startup_next_action.action_kind == "resume_required_return_task"` (`must_resume_required_return_task_before_unrelated_work = true`) или `execctl_active_lease.lease_owner_state == "previous_session_owner"` (`previous_session_owner_must_follow_startup_next_action = true`), follow startup_next_action first. `no_silent_drop = true`. Для resume смотри `execctl_active_lease_summary`, `required_return_task`, `required_task_set`, `required_task_set_summary`, `project_task_tree`, `project_task_tree_summary`, `project_task_ledger`, `project_task_ledger_summary`.
6. Перед каждым содержательным ответом обновляй guard `./scripts/client_budget_gate.sh` и работай только по `client_budget_reply_gate.reply_execution_gate`. `must_check_before_each_substantive_reply = true`; stale старше `10` секунд запрещён (`stale_guard_requires_refresh = true`). Hard gate automation: `--enforce-reply-gate` (`guard_enforcement_exit_on_blocking = true`). KPI/reply prefix сейчас отключён как обязательный startup-law (`required_reply_prefix_source = disabled_by_project_policy`, `required_reply_prefix_non_empty = false`, `reply_prefix_preflight_blocks_substantive_reply = false`, `output_prefix_enforcement_mode = disabled_by_project_policy`, `output_prefix_host_enforced = false`). Continuity write-side maintenance в Amai (continuity import, continuity handoff, observe /api/continuity-handoff) не блокируется reply guard (`continuity_write_exempt_from_reply_guard = true`) и при rotate/advisory pressure остаётся обязательным перед уходом (`continuity_write_required_before_rotate = true`). Для KPI/guard/exact-pair root-cause сначала используй `./scripts/client_budget_root_cause.sh`; `must_prefer_compact_diagnostics_over_full_snapshot = true`.
7. Gate version pinned: `client-reply-budget-gate-v1`. Поле `reply_execution_gate.reply_prefix` может по-прежнему materialize-иться для диагностики, но начинать user-visible reply с KPI-prefix больше не требуется и fail-closed preflight по нему отключён. Если `reply_budget_mode == "compact_high_signal"`, substantive reply разрешён только по `reply_budget_contract` с `contract_version = "client-reply-budget-v1"`: direct answer first, no unrequested recap, no repeated known context, keep only changed facts, prefer patch/result over narration when coding, preserve truthfulness/technical accuracy, disclose unknowns instead of guessing. Exact operator-switch для target режима: matching `^экономия_(0|10|20|30|40|50|60|70|80|90)%$` -> `./scripts/continuity_client_budget_target.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --percent N` (`repo_root_argument_required = true`, `switch_immediately_on_exact_chat_command = true`, `reply_with_confirmation_after_switch = true`). Пример exact chat-команды: `экономия_50%`. Exact operator-switch для huge-chat rebase: точную команду `компакт_чат` обработай через `./scripts/continuity_compact_chat.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --json` (`repo_root_argument_required = true`, `switch_immediately_on_exact_chat_command = true`, `reply_with_confirmation_after_prepare = true`, `prompt_text_required_for_rebase = true`), верни `prompt_text` и `operator_notice`, и требуй host action `open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable`.
8. Client-budget blocked reply mechanism removed: `reply_blocking_removed = true`. Tool-turn blocked mechanism removed too: `tool_turn_blocking_removed = true`. Если `reply_execution_gate.must_rotate_before_reply = true`, `reply_execution_gate.must_wait_for_budget_recovery_before_reply = true`, `should_rotate_chat_now = true`, `status_label` равен одному из current normalized same-thread advisory labels [сожми текущий чат, сожми текущий чат сейчас], `same_meter_pure_burn_turn_active = true`, `must_avoid_new_tool_turn_without_specific_delta_goal = true` или `max_tool_roundtrips_soft = 0`, считай это только advisory/compact pressure signal. Этот список в startup instructions является non-binding human-readable snapshot канонического shared advisory source, а не отдельным policy-list. User-visible blocked wait template использовать запрещено; `amai_context_pack`, continuity write и другие Amai tools не блокируй только из-за этих полей. `save_handoff_before_rotate = true` и `fresh_chat_requires_continuity_startup = true` остаются operator guidance.
9. Не подменяй полную клиентскую шкалу внутренним Amai-slice: `full_scale_client_truth_required = true`. Любой fail-closed scenario (project_unregistered, repo_root_binding_ambiguous, continuity_restore_unavailable) сообщай как блокер и не угадывай continuity.
<!-- /AMAI MANAGED STARTUP INSTRUCTIONS v1 -->

План materialization non-regression contract

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
