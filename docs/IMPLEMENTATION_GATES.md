# Карта проверок и дебаггинга по этапам

## Зачем нужен этот документ

Этот документ отвечает на один вопрос:
- как агент понимает, какими proof, verify, debug и reconcile механизмами проверять текущий этап.

Агент не должен угадывать это по названиям скриптов.

## Главный закон этого документа

Если для этапа уже существует готовый benchmark, proof-script или measured matrix harness, агент обязан сначала использовать именно его.

То есть:
- готовые benchmark/proof контуры обязательны;
- ad-hoc локальная проверка может быть только дополнением;
- локальная самопроверка не заменяет существующий benchmark-harness;
- если harness подходит этапу, без него этап нельзя честно закрывать.
- checkbox этапа нельзя закрывать после одного удобного blocking-proof;
- сначала обязателен весь уже materialized и подходящий для этапа набор harness;
- если изменение задевает соседний shared contour того же этапа
  (`speed / accuracy / isolation / truth / dashboard / continuity`),
  обязателен и companion non-regression harness для этого контура;
- любой harness, который используется для stage-close, обязан сам идти в полном benchmark режиме;
- если скрипт запускает урезанные `warmup/iterations`, это smoke-contour, а не stage-close proof;
- если benchmark публикует результат в dashboard, после прогона обязателен ещё и dashboard-check по соответствующей карточке или snapshot surface;
- canonical dashboard-check path:
  - использовать уже поднятый observe host и читать `/api/dashboard`;
  - для локального default bind это обычно `http://127.0.0.1:9464/api/dashboard`;
  - проверять именно matching `benchmark_cards[]`, а не гадать по сырым payload source lane;
- агент не имеет права ждать, пока пользователь отдельно напомнит про какой-то benchmark;
- если contour уже materialized и подходит touched surface, его надо запускать проактивно;
- правило простое:
  сначала полный подходящий stage bundle, потом решение о закрытии этапа.
- при каждом stage gate нужно проверять non-regression по 4 осям проекта:
  - скорость;
  - точность;
  - качество;
  - правдивость.
- если хотя бы одна ось просела, этап не закрывается:
  - сначала root-cause;
  - потом fix/recovery;
  - потом повторный benchmark/proof.

И ещё один жёсткий закон:
- для внутренних стадий проекта default verification/runtime language = `Rust`;
- если уже есть Rust-native contour, агент не имеет права обходить его новым `python`-harness “для удобства”;
- существующие `python`-пути допустимы только внутри уже materialized external benchmark compatibility paths.

## Как агент должен им пользоваться

Порядок такой:

1. Открыть `IMPLEMENTATION_STATUS.md`.
2. Определить текущий этап по checkbox и статусу.
3. Открыть соответствующий этап в `AMAI_GLOBAL_MEMORY_ROADMAP.md`.
4. Если изменение затрагивает critical zone или делает заметный refactor, пройти `./scripts/maintainability_gate.sh --json`.
4a. Если как часть значимого шага обновляется `docs/IMPLEMENTATION_STATUS.md`, после обновления пройти `./scripts/implementation_status_sync_guard.sh --json`.
4b. Если значимый этап собираются закрывать checkbox-ом, перед изменением checkbox пройти `./scripts/maintainability_stage_close_guard.sh --json`.
5. Открыть соответствующий раздел этого документа.
6. Использовать:
   - базовые механизмы;
   - stage-specific proof;
   - stage-specific debug/reconcile path.
   - существующие benchmark/proof harness раньше ad-hoc проверок.
7. Закрывать этап только после полного цикла:
   - tests;
   - весь подходящий stage-local и companion benchmark/proof bundle;
   - dashboard-check для всех benchmark contours, которые публикуются в dashboard;
   - manual check;
   - debug/fix;
   - retest.

## Канонический benchmark registry

Этот список нужен затем, чтобы агент не выбирал benchmarks по памяти и не ждал напоминаний пользователя.

### Retrieval / memory bundle

Если изменение задевает retrieval, scope filtering, visibility, relation graph, context pack, truth/artifact split, dashboard benchmark surface или любую memory-plane производительность, обязательны:
- `./scripts/proof_performance.sh`
- `./scripts/proof_accuracy.sh`
- `./scripts/proof_load.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_cold_benchmark.sh`
- `./scripts/proof_cold_benchmark_self_contained.sh`
- `./scripts/proof_cold_benchmark_canonical.sh`
- `./scripts/proof_memory_external_benchmarks.sh`
- dashboard-check через `/api/dashboard` для:
  - `Hot Load Benchmark / latest_retrieval_load_hot`
  - `Hot Retrieval Benchmark / latest_retrieval_hot`
  - `Cold End-to-End Benchmark / latest_cold_path_benchmark`
  - `Accuracy / Isolation Verification / latest_retrieval_accuracy`

### External vector / Qdrant bundle

Если изменение задевает vector path, semantic retrieval, Qdrant integration, external compare-layer или benchmark-Qdrant contour, обязательны:
- `./scripts/proof_external_benchmark_env.sh`
- `./scripts/proof_external_benchmark_adapter.sh`
- raw harvest/result verdict для `VectorDBBench / QdrantLocal`

Правило:
- если `benchmark_qdrant` surfaced в `/api/dashboard`, его надо перепроверять и там;
- если `benchmark_qdrant` сейчас `null`, нельзя делать вид, что dashboard-check был выполнен;
- в таком случае источником истины считается raw harvest/result verdict external contour.

### Token / compare bundle

Если изменение задевает compare-plane, token telemetry, savings surface или benchmark UX, обязательны:
- `./scripts/proof_token_benchmark.sh`
- `./scripts/proof_token_benchmark_suite.sh`
- `./scripts/proof_token_live_turn_savings_matrix.sh`
- `./scripts/proof_token_art_live_turn_savings.sh`
- `./scripts/proof_benchmark_matrix.sh`

### External memory benchmark registry (обязательно подключить)

Эти бенчи обязательны для зрелости memory-плана Amai. Пока они не materialized в repo,
нельзя закрывать этапы, где требуется долгосрочная память и агентная причинность.
Если harness ещё нет, добавляем его и фиксируем источники данных.

#### LongMemEval

Ссылки:
- GitHub: https://github.com/xiaowu0162/LongMemEval
- arXiv: https://arxiv.org/abs/2410.10813
- Project page: https://xiaowu0162.github.io/long-mem-eval/

Покрытие:
- information extraction
- multi-session reasoning
- temporal reasoning
- knowledge updates
- abstention

#### AMA-Bench (Agent Memory with Any length)

Ссылки:
- arXiv: https://arxiv.org/abs/2602.22769
- Dataset (HF): https://huggingface.co/datasets/AMA-bench/AMA-bench
- Leaderboard (HF Space): https://huggingface.co/spaces/AMA-bench/AMA-bench-Leaderboard

TODO:
- GitHub репозиторий (не найден в публичных источниках)
- Official project page (если есть отдельная)

Покрытие:
- long-horizon agent memory
- causality / objective information
- agent trajectories + tool/action streams

#### MemoryAgentBench

Ссылки:
- GitHub: https://github.com/HUST-AI-HYZ/MemoryAgentBench
- arXiv: https://arxiv.org/abs/2507.05257
- OpenReview: https://openreview.net/pdf?id=ZgQ0t3zYTQ

Покрытие:
- accurate retrieval
- test-time learning
- long-range understanding
- conflict resolution

#### LoCoMo

Ссылки:
- GitHub (data/code): https://github.com/snap-research/locomo
- arXiv: https://arxiv.org/abs/2402.17753

Покрытие:
- very long-term conversational memory
- temporal/causal dynamics across multi-session dialogs

## Где лежат механизмы проверки и что они значат

Агент не должен угадывать назначение инструмента по одному имени.

Ниже короткая карта:

### `scripts/proof_*.sh`

Где лежат:
- каталог `scripts/`

Что это:
- готовые product-proof сценарии;
- обычно проверяют один конкретный contour end-to-end;
- подходят как основной быстрый этапный proof.

Когда использовать:
- если этап уже имеет свой `proof_*.sh`;
- если нужно быстро проверить реальное пользовательское поведение, а не только unit-level кусок;
- если stage gate уже ожидает этот contour.

### `cargo run -- verify ...`

Где лежат:
- Rust CLI `amai`

Что это:
- более канонический verification/eval слой;
- measured проверки, матрицы и benchmark-контуры.

Когда использовать:
- если этап привязан к `verify continuity`, `verify accuracy`, `verify load`, `verify mcp-matrix`, `verify memory-matrix`, `verify token-benchmark` и похожим поверхностям;
- если нужен не только smoke proof, но и machine-readable verdict.

### `cargo run -- observe ...`

Где лежат:
- Rust CLI `amai`

Что это:
- live observability и debug surface;
- помогает понять текущее состояние runtime и причины решений.

Когда использовать:
- если надо понять не только `сломано/не сломано`, а что именно сейчас происходит;
- если нужен debug snapshot, SLA-check или explainability-layer.

### `scripts/continuity_*.sh`

Где лежат:
- каталог `scripts/`

Что это:
- специальные continuity/reconcile/debug инструменты;
- позволяют поднять startup, restore, answer, handoff и startup-state отдельно от всего остального.

Когда использовать:
- если этап затрагивает continuity, restore, chat transition, ExecCtl или startup contract;
- если нужно воспроизвести continuity contour руками и увидеть честный payload.

### `scripts/client_budget_*.sh`

Где лежат:
- каталог `scripts/`

Что это:
- отдельный debug/control слой для KPI, reply gate, compact mode, same-thread pressure и root-cause анализа.

Когда использовать:
- если работа касается `5ч KPI`, token cards, compact chat, reply gate или same-thread pressure.

### `scripts/status.sh`

Где лежит:
- каталог `scripts/`

Что это:
- самый короткий health-check stack/runtime состояния.

Когда использовать:
- почти всегда перед серьёзной проверкой;
- когда нужно быстро понять, жив ли стек и в каком он состоянии.

### `scripts/maintainability_gate.sh`

Где лежит:
- каталог `scripts/`

Что это:
- machine-readable binding внешнего maintainability/supportability/evolvability/anti-hardcoding стандарта к проекту `Amai`;
- быстрый fail-closed checklist для значимых изменений.

Когда использовать:
- перед stage-based реализацией;
- перед заметным refactor critical zone;
- если изменение трогает truth/policy/contracts/schema/recovery/observability;
- если есть риск нового hidden hardcode.

### `scripts/maintainability_stage_close_guard.sh`

Где лежит:
- каталог `scripts/`

Что это:
- machine-readable closure guard для checkbox значимого этапа;
- проверяет, что есть свежий maintainability gate trace;
- fail-closed, если после gate-run изменился `HEAD` или worktree fingerprint.

Когда использовать:
- прямо перед тем, как ставить checkbox значимого этапа;
- когда change set после последнего `maintainability_gate.sh` ещё менялся;
- когда нужно доказать, что checkbox ставится не по памяти агента, а по свежему gate-trace.

### `scripts/implementation_status_sync_guard.sh`

Где лежит:
- каталог `scripts/`

Что это:
- machine-readable sync guard для значимого обновления `docs/IMPLEMENTATION_STATUS.md`;
- проверяет, что текущий hash `docs/IMPLEMENTATION_STATUS.md` уже captured в свежем maintainability gate trace;
- fail-closed, если статус-документ менялся после gate-run или если после gate-run изменились `HEAD` и worktree fingerprint.

Когда использовать:
- после значимого обновления `docs/IMPLEMENTATION_STATUS.md`;
- перед тем как считать status snapshot честно обновлённым;
- когда нужно доказать, что status не менялся отдельно от свежего gate-trace.

### `scripts/agent_preflight.sh`

Где лежит:
- каталог `scripts/`

Что это:
- machine-readable входной gate для любого агента;
- собирает свежий JSON-срез:
  - какие документы обязательны;
  - какой этап сейчас активен;
  - что уже закрыто;
  - какой этап следующий;
  - какие ready-made механизмы уже подходят следующему этапу.

Когда использовать:
- до stage-based работы;
- когда агент не должен вычислять статус проекта по косвенным признакам;
- когда нужно fail-closed проверить, что onboarding corpus не разъехался.

## Как агент понимает, что брать для своего этапа

Простое правило выбора такое:

1. Сначала смотреть текущий этап в `IMPLEMENTATION_STATUS.md`.
2. Потом открыть этот этап в данном документе.
3. Если там уже перечислен готовый `proof_*.sh` или `verify ...`, брать его первым.
4. Если для этапа перечислены и `proof`, и `verify`:
   - `proof` использовать как быстрый product path;
   - `verify` использовать как более строгий measured verdict.
5. `observe ...` и debug scripts использовать не вместо proof, а чтобы понять причину проблемы.
6. `continuity_*` и `client_budget_*` брать тогда, когда этап касается именно этих контуров.

Итог:
- `proof` отвечает на вопрос `контур реально работает?`
- `verify` отвечает на вопрос `контур проходит измеряемую проверку?`
- `observe/debug/reconcile` отвечают на вопрос `почему он ведёт себя именно так?`

## Базовые механизмы для любого этапа

Это нужно почти всегда, независимо от стадии.

### Базовый health-check

- `./scripts/status.sh`
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`

### Базовый continuity/debug

Если работа касается continuity, restore, ExecCtl, startup или chat-transition:
- `./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json`
- `./scripts/continuity_startup_state.sh --repo-root "$(pwd)" --json`
- `./scripts/continuity_restore.sh`
- `./scripts/continuity_handoff.sh`

### Базовый budget/debug

Если работа касается KPI, chat pressure, compact mode, reply gate или token cards:
- `./scripts/client_budget_gate.sh`
- `./scripts/client_budget_root_cause.sh`
- `./scripts/client_budget_system_markers.sh`

### Базовый observability/debug

Если нужно понять, почему система решила именно так:
- `./scripts/proof_observability.sh`
- `./scripts/proof_context_decision_trace.sh`
- `./scripts/proof_working_state_decision_trace.sh`

## Этап 0. Общая модель memory fabric

Смысл этапа:
- согласовать документы;
- развести current-state и target-state;
- зафиксировать канонический порядок.

### Чем проверять

Это docs-first этап.

Для него нужны:
- cross-doc review;
- ручная проверка ссылок и терминов;
- `./scripts/repo_hygiene_guard.sh --json`
- continuity handoff после правок.

### Чем дебажить

- `git diff`
- ручная сверка `AGENTS / README / ARCHITECTURE / OPERATIONS / ROADMAP / STATUS`

### Когда этап можно закрыть

Только когда:
- документы перестали спорить друг с другом;
- появился единый onboarding path;
- current-state и target-state разведены явно.

## Этап 1. Scope и identity control plane

Смысл этапа:
- materialize-ить `workspace / project / project_link / scope_type / права / import flow`.

### Чем проверять

Главные proof:
- `./scripts/scope_identity_surface_guard.sh --json`
- `./scripts/proof_scope_identity_control_plane.sh`
- `./scripts/proof_project_registration_canonicalization.sh`
- `./scripts/proof_project_relocation_contour.sh`
- `./scripts/proof_hostile.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_accuracy.sh`
- `./scripts/proof_performance.sh`
- `./scripts/proof_cold_benchmark.sh`
- `./scripts/proof_cold_benchmark_self_contained.sh`
- `./scripts/proof_cold_benchmark_canonical.sh`
- `./scripts/proof_load.sh`
- `./scripts/proof_external_benchmark_env.sh`
- `./scripts/proof_external_benchmark_adapter.sh`

### Чем дебажить

- `./scripts/status.sh`
- `cargo run -- project ...`
- `cargo run -- namespace ...`
- `cargo run -- relation ...`
- `cargo run -- observe snapshot`
- `curl -sf <observe_base_url>/api/dashboard`

### Что особенно смотреть

- unrelated projects действительно изолированы;
- illegal import fail-closed;
- project relocation не ломает identity;
- права не дают тихого permissive fallback.
- accuracy/isolation contour не просел после изменений scope/visibility;
- retrieval performance contour не просел после изменений scope/visibility.
- cold micro contour и full end-to-end cold contour оба должны быть проверены; нельзя закрывать этап по одному из них.
- hot-load contour не просел после изменений scope/visibility и relation graph.
- если `proof_memory_task_matrix.sh` красный только по latency, а correctness остаётся `1.0 / 1.0`, этап всё равно нельзя закрывать: сначала нужен root-cause и восстановление speed baseline.
- если `proof_performance.sh` или другой stage-close benchmark шёл на урезанных `warmup/iterations`, этап всё равно нельзя закрывать: сначала нужен полный режим не ниже publish-требований dashboard/SLA.
- если benchmark contour уже публикуется на dashboard, stage-close без проверки соответствующей dashboard-card/snapshot запрещён.
- для cold-path это значит проверять три вещи вместе:
- для reproducibility / clean-machine story это значит проверять четыре вещи вместе:
  - `retrieval_benchmark_cold` через `./scripts/proof_performance.sh`;
  - proof full-cold contour через `./scripts/proof_cold_benchmark.sh`;
  - repo-local self-contained contour через `./scripts/proof_cold_benchmark_self_contained.sh`;
  - canonical dashboard-driving cold contour через `./scripts/proof_cold_benchmark_canonical.sh`, а затем ещё и `Cold End-to-End Benchmark / latest_cold_path_benchmark`.
- если touched surface включает vector/Qdrant lane, нельзя пропускать external bundle:
  - `./scripts/proof_external_benchmark_env.sh`;
  - `./scripts/proof_external_benchmark_adapter.sh`;
  - raw harvest/result verdict для `VectorDBBench / QdrantLocal`;
  - `benchmark_qdrant` на dashboard проверяется только если он реально surfaced и не `null`.

## Этап 2. Typed memory envelope + provenance

Смысл этапа:
- сделать единый durable contract для memory objects;
- прикрутить source links, trust state и temporal truth.

### Чем проверять

Главные proof:
- `./scripts/typed_memory_envelope_guard.sh --json`
- `./scripts/proof_typed_memory_envelope_contract.sh`
- `./scripts/proof_context_decision_trace.sh`
- `./scripts/proof_working_state_decision_trace.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_observability.sh`

### Чем дебажить

- `./scripts/proof_stage2_setup.sh`
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`
- explainability traces из decision/provenance paths

### Что особенно смотреть

- summary не становится truth без evidence;
- provenance поля не пустые;
- `memory_envelopes` view остаётся canonical typed contract, а не случайным projection drift;
- `retrieval_traces` materialize `candidate_summary / rerank_summary / evidence_sufficiency / final_decision`, а не остаются пустой schema-заготовкой;
- `decision_trace` честно показывает `evidence_ladder` и `cheapest_sufficient_layer`, а не только flat hit-list;
- read path materialize-ит literal pipeline stages `intent_classifier / scope_resolver / candidate_generation / rerank_legality_relevance / escalate_if_needed / abstain_if_insufficient`;
- provenance details materialize-ят explicit `write_pipeline`, а не только набор полей без operational trace;
- `verified_write_back` не materialize-ится без verified trust/evidence и явного escalation trail к raw/artifact/log/temporal source;
- `observed_at / recorded_at / valid_from / valid_to` не теряются;
- conflict path не превращается в silent overwrite.

## Этап 3. Commitment / task graph

Смысл этапа:
- перевести task-memory из tree-first остатка в честный graph-first operational layer.

### Чем проверять

Главные proof:
- `./scripts/proof_execctl_pending_return.sh`
- `./scripts/proof_execctl_restore_stress.sh`
- `./scripts/proof_execctl_resolved_task_ids.sh`
- `./scripts/proof_execctl_resolved_task_identity.sh`
- `./scripts/proof_commitment_task_graph_integrity.sh`

### Чем дебажить

- `./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json`
- `./scripts/continuity_startup_state.sh --repo-root "$(pwd)" --json`
- `cargo run -- observe snapshot`

### Что особенно смотреть

- pending return не теряется;
- resolved tasks не путаются по identity;
- graph edges не ломают resume obligations;
- lease/task truth не живёт только в одном restore-window.
- task graph не плодит дубли по `task_key`;
- `resumed/reopened` materialize-ятся обратно в текущий `task_node`, а не остаются только в event-log;
- `memory_link_decision` не остаётся только explainability-table:
  - `continue / child / new` должны давать честный graph-effect на `task_node/task_event`;
  - `abstain / escalate / pending_link_proposal` не должны теряться между decision-table и task-graph;
  - `pending_link_proposal` через `memory_link_decision` обязан оставлять `evidence_request` с `pending_link_ttl_epoch_ms` и `additional_evidence_request`;
- `pending_link_proposal` при low-confidence routing должен оставлять `evidence_request` на task-graph слое, а не висеть только отдельной truth-записью;
- `open / closed / archived` читаются честно на current-state слое.

## Этап 3A. Ранний procedural seed contour

Смысл этапа:
- завести первые `candidate skill` и `shadow mode`, но не выпускать их как зрелую shared truth.

### Чем проверять

Уже materialized:
- `./scripts/proof_procedural_seed.sh`
- `./scripts/proof_procedural_shadow_review.sh`
- `./scripts/proof_restore_execution_card.sh`
- `./scripts/proof_shared_promotion_by_approval.sh`
- `./scripts/review_procedural_shadow_mode.sh`

Но:
- этап всё ещё нельзя честно закрыть без shadow-mode review и stage-close non-regression bundle.

Companion minimum:
- `./scripts/proof_observability.sh`
- evaluator/debug traces (`amai skill review --skill-card-id ...`)
- manual restore review (`amai continuity restore --project ... --namespace ... --json`)
- manual shadow-mode review (`./scripts/review_procedural_shadow_mode.sh amai continuity`)

### Чем дебажить

- traces по skill extraction;
- evaluator verdict logs;
- continuity handoff с явным evidence gap.

### Что особенно смотреть

- skill не промотится сразу в verified/shared;
- shadow mode ничего не ломает в live execution;
- плохой candidate skill не проходит как improvement.
- если procedural memory уже materialized, restore поднимает компактную `execution card`, а не сырую длинную заметку.

## Этап 4. Workspace restore pack

Смысл этапа:
- заменить слишком узкий task-centric restore на полноценный рабочий пакет.

### Чем проверять

Главные proof:
- `./scripts/proof_art_continuity_startup.sh`
- `./scripts/proof_art_continuity_restore.sh`
- `./scripts/proof_workspace_restore_pack_acceptance.sh`
- `./scripts/proof_workspace_restore_pack_hardening.sh`
- `./scripts/proof_token_continuity_restore_observed.sh`
- `cargo run --quiet -- verify continuity --project art --namespace continuity`

### Чем дебажить

- `./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json`
- `./scripts/continuity_startup_state.sh --repo-root "$(pwd)" --json`
- `./scripts/continuity_restore.sh`

### Что особенно смотреть

- в restore есть не только задачи;
- `blocked/waiting` не остаются пустой декларацией:
  - должен быть отдельный acceptance-proof, где этот bucket непустой и честно surfaced;
- startup и restore не расходятся;
- следующий чат поднимает полезный рабочий пакет, а не пустой summary.
- если procedural memory materialized:
  - `workspace_restore_pack` обязан surface-ить именно `compact execution card`, а не сырой procedural архив.
- stale replay, poisoned evidence span и mismatched/missing source snapshot должны падать fail-closed, а malformed restore surface не должен silently превращаться в ложную рабочую картину.

## Этап 5. Semantic + temporal memory strengthening

Смысл этапа:
- усилить factual/temporal retrieval, exact lookup и historical truth.

### Чем проверять

Главные proof:
- `./scripts/proof_semantic_temporal_memory.sh`
- `./scripts/proof_semantic_temporal_manual_acceptance.sh`
- `./scripts/proof_accuracy.sh`
- `./scripts/proof_text_compare.sh`
- `./scripts/proof_text_compare_real_projects.sh`
- `cargo run --release -- verify accuracy ...`

### Чем дебажить

- `./scripts/proof_context_decision_trace.sh`
- `./scripts/proof_working_state_decision_trace.sh`
- retrieval debug через `observe snapshot`

### Что особенно смотреть

- temporal fail-closed не превращается в гадание;
- lexical/exact retrieval не вытесняется embeddings;
- related-project path не ломает project boundaries.
- `knowledge update` действительно supersede-ит старый current fact при смене факта, а не оставляет два current-state знания рядом;
- exact-time slice поднимает исторически валидный факт, а latest-state path не подмешивает его назад.
- generic factual NL query без искусственного lexical anchor всё ещё поднимает semantic fact и не тащит future-only формулировку назад в старый time-slice;
- `verify_context_pack` не переиспользует stale fast-cache после relation/access-policy change.
- `retracted` truth-state закрывает temporal window на write-path и перестаёт быть видимым и в latest path, и в future slice после момента retract.
- latest factual retrieval не отдаёт `conflicted/disputed` claim выше `current+verified` факта только потому, что conflicted card новее или текстово длиннее.

## Этап 6. Multi-agent shared/private memory

Смысл этапа:
- materialize-ить shared/private memory без утечек и без тихой путаницы между агентами.

### Чем проверять

Главные proof:
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_hostile.sh`
- `./scripts/proof_load.sh`
- `./scripts/proof_shared_private_memory_hardening.sh`
- `./scripts/proof_shared_private_memory_manual_acceptance.sh`

### Чем дебажить

- `cargo run -- observe snapshot`
- `./scripts/status.sh`
- isolate/relation/project debug surfaces

### Что особенно смотреть

- shared memory не ломает private boundaries;
- unrelated scopes не читают друг друга;
- concurrent load не портит truth;
- multi-agent isolation сохраняется под stress.
- `org_global` не живёт на implicit fallback: transfer policy обязателен, binding остаётся same-workspace only, а duplicate `shared_asset.code` в соседнем workspace не должен менять target binding contour.
- borrowed import path не может выдавать себя за local truth: до verified local copy он обязан оставаться `visibility_scope = imported` и `proposed/unverified`.

## Этап 7. Compare + benchmark plane

Смысл этапа:
- честно мерить `с Amai / без Amai` и materialize-ить benchmark surface.

### Чем проверять

Главные proof:
- `./scripts/proof_token_benchmark.sh`
- `./scripts/proof_token_benchmark_suite.sh`
- `./scripts/proof_token_live_turn_savings_matrix.sh`
- `./scripts/proof_token_art_live_turn_savings.sh`
- `./scripts/proof_benchmark_matrix.sh`
- `./scripts/proof_procedural_benchmark.sh`

### Чем дебажить

- `./scripts/client_budget_root_cause.sh`
- `./scripts/client_budget_system_markers.sh`
- `cargo run -- observe snapshot`

### Что особенно смотреть

- live lane не загрязняется proof/verify;
- exact pair действительно честный;
- benchmark и online не смешиваются;
- current-session и rolling-window не врут.
- procedural benchmark живёт как first-class compare contour, а не как текстовая ссылка на будущую идею;
- procedural benchmark не подменяет stage-local architectural proof:
  - он обязан собирать reuse/suppression/uplift/evaluator метрики поверх уже существующих truth-layer proofs;
- benchmark catalog явно surface-ит `Навыки и память действий` с отдельным entrypoint и explain surface.
- если `without Amai` procedural line ещё не materialized, compare-plane обязан fail-closed surface-ить partial benchmark-state и `not_materialized` line summary вместо guessed second line.
- после materialization `without_amai_but_measuring` надо отдельно проверить dual-line payload:
  - `benchmark_without_amai_series` non-empty;
  - `benchmark_line_summaries.without_amai_but_measuring.state = materialized`;
  - dashboard-card больше не висит в `waiting`.
- richer compare reporting теперь обязан поднимать persisted history отдельно от latest snapshot:
  - `procedural_benchmark_history.history_rows`;
  - `with_amai_pass_percent_series`;
  - `without_amai_pass_percent_series`.

## Этап 8. Procedural memory

Смысл этапа:
- довести skill memory до полноценного durable/evaluated/shared слоя.

### Чем проверять

Текущий честный статус:
- dedicated full-stage harness уже materialized на compare-plane как `./scripts/proof_procedural_benchmark.sh`;
- richer scored reporting и persisted benchmark history тоже уже materialized:
  - `observe snapshot` поднимает `procedural_benchmark_history`;
  - compare payload держит `dual_line_materialized`;
  - линия `without_amai_but_measuring` уже surfaced как честная benchmark-линия, а не pending placeholder.

Значит:
- этап нельзя честно закрыть без passing отдельного procedural benchmark/proof и companion evaluator/trust traces.

Минимум, который потребуется:
- отдельный procedural benchmark из compare-plane;
- evaluator/trust verification;
- shadow -> trial -> verified trace review.

### Чем дебажить

- skill extraction/eval traces;
- `./scripts/proof_observability.sh`
- compare/debug surfaces для skill reuse.

### Что особенно смотреть

- skill pool не становится append-only складом;
- плохие навыки уходят в quarantine/deprecated;
- shared procedural memory не растёт без trust gate.
- `project_shared` skill не имеет права попасть в `execution-card` только из-за `promote_verified`;
- до explicit evaluator/trust verdict `approve_shared_promotion` shared skill обязан оставаться в `pending_approval`;
- после approval надо перепроверять и `skill review`, и `skill execution-card`, чтобы увидеть переход `0 -> 1` на shared surface.

## Этап 9. Forgetting, consolidation, pruning

Смысл этапа:
- научить память не только копить, но и честно чистить, сворачивать и архивировать.

### Чем проверять

Главные текущие best-fit proof:
- `./scripts/proof_forgetting_consolidation.sh`

Companion non-regression:
- `./scripts/proof_observability.sh`

### Чем дебажить

- `cargo run -- memory explain-forgetting --memory-item-id ...`
- `cargo run -- memory run-job --job-kind ...`
- `/api/dashboard` и governance-card `Жизненный цикл памяти`
- observability snapshot;
- forgetting audit log / retention traces;
- raw `ami.memory_items` consolidation_status / retention_class / decay_policy.

### Что особенно смотреть

- pruning не убивает truth;
- archive path объясним;
- named forgetting jobs surfaced как first-class runtime contour, а не живут только в roadmap;
- `summarization_job` либо explicit no-op, либо с доказанным effect, но не silent missing branch;
- governance/dashboard surface показывает breakdown forgetting jobs, а не только aggregate audit count;
- stale current knowledge уходит в `pending_review`, а не продолжает жить как fresh;
- `raw_capture / operator_write / verified_write_back / durable / legal_hold / retain_forever` не попадают под auto-prune/archive;
- stale/live contamination не возвращается после cleanup.

## Этап 10. Governance, safety, evaluator loop

Смысл этапа:
- закрыть safety, poisoning, privacy, evaluator memory и final governance layer.

### Чем проверять

Главные proof:
- `./scripts/proof_hostile.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_observability.sh`

Дополнительно смотреть:
- token-contract proofs;
- privacy/safety specific traces;
- quarantine and override paths.

### Чем дебажить

- `cargo run -- observe snapshot`
- hostile and isolation traces;
- conflict/quarantine/audit surfaces

### Что особенно смотреть

- poisoning не проходит как нормальная эволюция памяти;
- privacy rules не ломаются shared/import paths;
- evaluator контур реально останавливает плохие “улучшения”.

## Честное правило про evidence gap

Если у этапа ещё нет dedicated proof harness, агент не имеет права закрывать этап “по ощущениям”.

Тогда он обязан:
- явно назвать evidence gap;
- не ставить checkbox;
- добавить создание proof/benchmark в обязательную часть этого этапа.
