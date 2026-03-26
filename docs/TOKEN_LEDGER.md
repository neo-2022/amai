modified_at: 2026-03-26 15:27 MSK
Ручная сверка guide/docs: 2026-03-26 15:27 MSK

# Token Ledger

Этот документ фиксирует канонический способ считать token-efficiency в `Amai`.

Главная идея простая:
- важна не просто “сырая экономия токенов”;
- важна честная экономия, которая:
  - считается только на реалистичном baseline;
  - не ухудшает качество;
  - штрафуется, если потом пришлось доуточнять, делать fallback или correction.

Главный продуктовый KPI поэтому называется:

`Verified Effective Savings %`

## Что именно должен уметь ledger

Ledger обязан отвечать на три вопроса:

1. Сколько токенов реально сэкономлено.
2. Сохранилось ли качество.
3. Не были ли savings потом “съедены назад” follow-up-ами, retry или fallback.

Если третий пункт не учтён, цифра считается неполной.

## Канонический смысл метрики

`Verified Effective Savings %` значит:
- только `live`-трафик;
- только события, где качество прошло gate;
- с учётом recovery penalties;
- агрегирование по сумме токенов, а не по среднему от процентов.

Это и есть headline-метрика для обычного пользователя.

## Что считается baseline

Baseline должен быть реалистичным.

Разрешённые baseline-классы:
- `naive_top_files`
- `grep_top_files`
- `ide_search_top_files`
- `semantic_top_k`
- `legacy_pre_amai`

В текущем runtime `Amai` уже старается выбирать baseline class по типу вопроса, а не держать почти всё на одном `naive_top_files`:
- `ide_search_top_files` для file/config/symbol lookup;
- `semantic_top_k` для architecture/bugfix path;
- `legacy_pre_amai` для onboarding path.

Запрещённые baseline-классы:
- `entire_repo`
- `all_docs`
- любой искусственно раздутый scope, который реальный агент обычно не отправил бы модели.

Иначе savings будут красивыми, но нечестными.

## Обязательное разделение трафика

Ledger не имеет права смешивать:
- `live`
- `verify`
- `proof`
- `benchmark`

По умолчанию product-report использует только:
- `traffic_class = live`

Все остальные режимы разрешены только как отдельный engineering view.

Для прямого `context pack` это теперь управляется и на уровне event contract:
- обычный пользовательский вызов по умолчанию пишет `source_kind = live_context_pack`;
- инженерные прогоны обязаны идти через `proof_*` или `verify_*` source kind;
- для этого у CLI теперь есть явный флаг:
  - `--token-source-kind proof_context_pack`
  - `--token-source-kind verify_context_pack`

Это важно не как косметика, а как truth guardrail:
- новый proof-run больше не должен сам по себе загрязнять live tokenonomics;
- `verify *` CLI и MCP proof-session теперь должны сами переписывать забытый default
  `live_context_pack` в non-live engineering source kind, чтобы proof contamination не ломал
  operational live cards из-за пропущенного аргумента;
- `verify memory-matrix` теперь тоже обязан маркировать свои archival context-pack события как
  `verify_memory_matrix_context_pack`, иначе matrix/eval-трафик будет ошибочно портить live
  карточки текущей сессии и рабочего окна;
- если старая текущая сессия уже успела нахватать `live_context_pack` ещё до этого разделения,
  она честно остаётся загрязнённой до нового session window или явного repair/reverify path;
- этот явный repair path теперь умеет не только legacy shape-upgrade, но и operator-driven
  source-kind rewrite по selector-ам `project/project_prefix + namespace + source_kind`;
- rewrite path обязан оставлять audit след в `token_budget_event.repair.operator_source_kind_rewrite`
  и не имеет права молча переписывать весь rolling window без явного operator selector-а;
- если destructive rewrite не делался, но для того же
  `project + namespace + measurement_scope + correlation_id` уже появился более новый non-live
  sibling (`proof_* / verify_* / benchmark_*`), report-layer теперь обязан fail-closed выкинуть
  старое `live_context_pack` из product views;
- это нужно затем, чтобы same-meter и live savings не продолжали считать stale contamination
  только потому, что engineering repair materialized как отдельный snapshot, а не как update
  старого live payload;
- тихо переписывать такую историю задним числом запрещено.

## Обязательные поля события

Каждый retrieval event должен иметь хотя бы:
- `event_id`
- `correlation_id`
- `timestamp_utc`
- `occurred_at_epoch_ms`
- `ingested_at_epoch_ms`
- `session_id` или эквивалент session grouping
- `rolling_window_profile`
- `traffic_class`
- `measurement_scope`
- `project_code`
- `namespace_code`
- `query_hash`
- `query_type`
- `cold_warm_state`
- `baseline_strategy`
- `baseline_tokens`
- `delivered_tokens`
- `saved_tokens`
- `gross_savings_pct`
- `recovery_tokens`
- `effective_saved_tokens`
- `effective_savings_pct`
- `quality_ok`
- `quality_score`
- `quality_method`
- `fallback_triggered`
- `fallback_count`
- `latency_ms`
- `sources_count`
- `chunks_count`

Для billing-grade эволюции событие теперь должно нести и contract-версии, даже если
денежный режим ещё работает только как `report_only`:
- `usage_event_schema_version`
- `metering_event_schema_version`
- `usage_lifecycle_model_version`
- `baseline_method_version`
- `quality_method_version`
- `coverage_model_version`
- `excluded_taxonomy_version`
- `dedup_contract_version`
- `backfill_policy_version`
- `correction_policy_version`
- `event_time_policy_version`
- `billing_policy_version`
- `billing_mode`
- `reconciliation_contract_version`
- `margin_model_version`
- `infra_cost_profile_version`
- `rate_card_version`
- `currency_profile`
- `settlement_status`

## Usage event schema и lifecycle

Сейчас у ledger уже должен быть не просто набор token-полей, а канонический usage-event
contract.

Что это значит practically:
- у события есть `usage_identity`;
- у события есть `usage_state`;
- report отдельно публикует `usage_event_schema`.

`usage_identity` обязан отвечать на вопрос:
- что является канонической единицей usage;
- по какому ключу она дедупится;
- какое время используется для расчётных окон.

Минимальный contract:
- `event_id`
- `correlation_id`
- `source_kind`
- `traffic_class`
- `project_code`
- `namespace_code`
- `measurement_scope`
- `occurred_at_epoch_ms`
- `ingested_at_epoch_ms`
- `dedup_key = source_kind:event_id`

`usage_state` обязан отвечать на вопрос:
- вошло ли событие в verified rollup;
- если нет, то почему;
- какой у него lifecycle status;
- это live ingest, legacy ingest или reverified backfill.

Текущие канонические lifecycle statuses:
- `verified_included`
- `excluded_quality_gate_failed`
- `excluded_awaiting_followup_reconciliation`
- `excluded_legacy_unverified`
- `excluded_non_live`

Текущие reporting layers на event level:
- `measured_non_billable`
- `excluded`

Это честно соответствует текущему режиму продукта:
- metering уже сильный;
- billing semantics ещё не включены;
- money-facing settlement пока не materialized.

В текущем runtime `Amai` старается писать эти канонические поля прямо в событие, а не держать их только как внутренние alias:
- `project_code`
- `namespace_code`
- `baseline_tokens`
- `delivered_tokens`
- `gross_savings_pct`

Сильно желательные поля:
- `target_kind`
- `baseline_hit_target`
- `amai_hit_target`
- `head_hit_target`
- `quality_tier`
- `needed_followup`
- `followup_count`
- `source_breakdown`
- `symbol_hits`
- `document_hits`
- `file_hits`
- `pack_token_count`
- `deduped_token_count`
- `budget_profile`
- `model_class`

## Как понимать главные поля

`baseline_tokens`
- сколько токенов пришлось бы передать модели без `Amai`, но через реалистичный baseline.

`delivered_tokens`
- сколько токенов реально передал `Amai`.

`saved_tokens`
- сырая экономия до штрафов.

Формула:

`max(0, baseline_tokens - delivered_tokens)`

`recovery_tokens`
- токены, которые пришлось потратить потом, потому что первого ответа/контекста не хватило.

Сюда входят:
- fallback retrieval;
- follow-up context;
- correction-turn context;
- retry context.

`effective_saved_tokens`
- реальная экономия после recovery penalties.

Формула:

`baseline_tokens - (delivered_tokens + recovery_tokens)`

`quality_ok`
- главный gate.
- `true` только если `Amai` не ухудшил пригодность результата относительно заданного quality threshold.

## Как считать aggregate-метрики

Правильно:

`total_saved_tokens = sum(saved_tokens_i)`

`gross_savings_pct = sum(saved_tokens_i) / sum(baseline_tokens_i)`

`effective_savings_pct = sum(effective_saved_tokens_i) / sum(baseline_tokens_i)`

Запрещено:

`avg(gross_savings_pct_i)`

То есть проценты нельзя усреднять по событиям напрямую.

## Quality gate

Есть три уровня строгости:

1. `Retrieval parity`
   - найден нужный файл, символ, документ или evidence-bundle.
2. `Answer parity`
   - ответ на базе `Amai` не хуже ответа на базе baseline retrieval.
3. `Human usefulness`
   - человек подтвердил, что критичного пропуска не было и лишний follow-up не понадобился.

Минимальное production-правило:
- headline KPI можно считать только по событиям, где `quality_ok = true`.

При этом полезно держать более честный градиент качества, а не только boolean:
- `retrieval`
  - цель формально найдена;
- `answer_proxy`
  - найденный контекст уже выглядит достаточным для полезного ответа без лишнего follow-up;
- `task_proxy`
  - legacy-compatible более строгий proxy поверх retrieval parity;
- `answer_success_recovered`
  - answer-like результат достигнут через recovery chain и уже учёл penalty за промах;
- `task_success_recovered`
  - legacy-compatible recovered task-like результат;
- `partial`
  - есть зацепки, но до quality gate ещё не дотянуто.

Практически полезная secondary-метрика поверх этого градиента:
- `verified_answer_like_savings_pct`
- `answer_like_rate`
- `verified_task_like_savings_pct`

Смысл:
- headline всё ещё остаётся `Verified Effective Savings %`;
- `verified_answer_like_savings_pct` показывает более строгую долю savings по событиям, где контекст уже дошёл до answer-like proxy;
- но `verified_task_like_savings_pct` показывает более строгую долю savings по событиям, которые уже дошли до `task_proxy` или `task_success_recovered`.

## Recovery penalties

Если `Amai` сначала сэкономил токены, но потом вызвал:
- follow-up;
- retry;
- fallback;
- correction;

то эти токены обязаны вычитаться из savings.

Именно поэтому `effective` важнее `gross`.

Для live report действует ещё одно правило:
- один follow-up должен штрафовать только ближайшее подходящее незакрытое событие;
- нельзя раздувать penalty на несколько старых событий сразу.

Если успешный follow-up реально исправил предыдущий промах, допустим более сильный runtime label:
- `hybrid_task_success`
- `hybrid_answer_success`

Смысл по-человечески:
- сначала был недостаточный retrieval;
- потом был follow-up;
- и именно второй шаг довёл задачу до полезного результата, но уже с recovery penalty.

## Сессии и окна

По умолчанию сессия — это непрерывная работа без паузы дольше `30 минут`.

Обязательные rollup-уровни:
- `current_session`
- `rolling_window`
- `lifetime`

Рекомендуемые rolling windows:
- `5h`
- `24h`

Каждый rollup должен показывать:
- `events_count`
- `baseline_tokens`
- `delivered_tokens`
- `saved_tokens`
- `gross_savings_pct`
- `recovery_tokens`
- `effective_saved_tokens`
- `effective_savings_pct`
- `quality_ok_rate`
- `fallback_rate`

## Coverage и excluded taxonomy

Накопительная savings-цифра без coverage считается неполной.

Каждый rollup теперь обязан публиковать отдельный `coverage` слой:
- `model_version`
- `completeness_state`
- `measured_events`
- `included_events`
- `excluded_events`
- `event_coverage_pct`
- `measured_baseline_tokens`
- `included_baseline_tokens`
- `excluded_baseline_tokens`
- `baseline_token_coverage_pct`

Смысл по-человечески:
- `measured_events`
  - всё, что ledger уже увидел в этом scope;
- `included_events`
  - что реально вошло в главный verified итог;
- `excluded_events`
  - что измерено, но не может честно попасть в headline;
- `completeness_state`
  - не “зелёный/красный статус”, а степень завершённости измерения.

Разрешённые состояния сейчас:
- `empty`
- `no_confirmed_usage`
- `partially_confirmed`
- `fully_confirmed`

Рядом с coverage обязан жить и `excluded_breakdown`.
Это не мусорный хвост, а честная причина, почему часть измеренного потока не попала в
главный итог.

Текущая каноническая taxonomy:
- `quality_gate_failed`
- `awaiting_followup_reconciliation`
- `legacy_unverified`
- `synthetic_verify`
- `synthetic_proof`
- `synthetic_benchmark`
- `non_live_other`

Для каждого excluded-класса нужно видеть:
- `events_count`
- `baseline_tokens`
- `delivered_tokens`
- `recovery_tokens`
- `effective_saved_tokens`

Эти коды нельзя silently переписывать задним числом.
Если старое событие было записано по старой schema/version, оно должно сохранять свою
историческую правду, даже если новый report уже считает поверх него более сильные
aggregate semantics.

## Whole-agent-cycle lower bound и reporting layers

`agent_cycle_economics` нельзя подавать как “весь бюджет всей сессии”.

Канонический truth guardrail теперь такой:
- `retrieval savings floor` реален;
- `partial whole-agent-cycle lower bound` реален;
- `full session economics` пока ещё не полностью измерен.

Поэтому `agent_cycle_economics` обязан публиковать не только timeline и lower bound, но и
reporting layers:
- `billable`
- `measured_non_billable`
- `unmeasured`

В текущем runtime это честно materialized как:
- `billable.status = disabled_report_only`
- `measured_non_billable.status = active`
- `unmeasured.status = active`

То есть денежный режим ещё не включён. Пока это report-only lower bound со строгим
разделением уже измеренной и ещё не измеренной части цикла.

При этом whole-cycle measurement теперь можно materialize-ить не только целиком или никак.
Ledger обязан различать:
- полностью не наблюдаемые компоненты;
- частично наблюдаемые whole-cycle компоненты;
- whole-cycle observed компоненты при всё ещё partial baseline.

Начиная с `client-limit-meter-alignment-v9` `client_prompt` считается observed component
не только если он явно пришёл в `whole_cycle_observed`, но и как derived fallback из
уже записанных `query + tokenizer`. Это нужно затем, чтобы progress к client-limit meter
был виден честно даже на исторических live events, где старый payload ещё не нёс
отдельное поле `client_prompt_tokens`.

Именно поэтому `client_limit_meter_alignment` теперь должен публиковать ещё и:
- `partially_measured_components`;
- `not_applicable_components`;
- `component_event_coverage`;
- state `whole_cycle_partially_observed_not_meter_equivalent`;
- state `whole_cycle_observed_baseline_partial`.
- state `whole_cycle_observed_explicit_boundary_not_meter_equivalent`.
- `assistant_generation_observation_source`.
- `baseline_equivalence`.

Это нужно затем, чтобы progress к реальному client-limit meter был виден честно, без
ложного переключения `same_meter_as_client_limit=true` раньше времени.

`baseline_equivalence` теперь обязан быть versioned и machine-readable:
- `model_version = client-limit-baseline-equivalence-v3`
- `state`
- `remaining_gap_reason`
- `applicable_components / fully_observed_components / incomplete_components`
- `measured_baseline_components / explicitly_unmodeled_baseline_components / missing_baseline_components`
- `measured_baseline_tokens_lower_bound`
- `strict_client_meter_slice`
- `explicit_boundary_surface`
- `continuity_boundary_rollup`
- `whole_cycle_components_fully_observed`

Это нужно затем, чтобы `same_meter_baseline_unmeasured /
same_meter_baseline_partially_measured / same_meter_baseline_explicit_boundary`
не оставались только строками в `blocking_reasons`, а baseline-gap был отдельным
truthful/measured contour:
что whole-cycle observed слой уже дотянулся до applicable components, какие из них уже
получили baseline-equivalent semantics, а какие обязаны оставаться явной truth-boundary
без guessed pre-Amai baseline.

Рядом с этим обязан жить отдельный versioned strict same-meter slice:
- `model_version = client-limit-strict-meter-slice-v1`
- `state`
- `lower_bound_tokens`
- `components`
- `explicit_boundary_components / missing_components`

Он нужен затем, чтобы already-measured same-meter-equivalent lower bound не терялся внутри
общего non-equivalent state. Если `client_prompt` уже truthful passthrough, operator должен
видеть этот strict slice отдельно, даже когда полный contour ещё заблокирован.

Рядом с ним теперь обязан жить и отдельный versioned explicit boundary surface:
- `model_version = client-limit-explicit-boundary-surface-v1`
- `state`
- `components`
- `note`

Он нужен затем, чтобы explicit continuity boundary не терялась внутри общего
`baseline_equivalence` и не выглядела как обычный missing implementation gap.

Dashboard hero-cards теперь обязаны поднимать рядом ещё и отдельный user-facing row
`Токены continuity boundary`, если boundary-state = `amai_continuity_boundary` и observed
tokens для этого компонента уже есть.

Он нужен затем, чтобы Amai-specific continuity boundary была видна не только как причина
non-equivalent state, но и как отдельный non-client-meter token rollup.

Этот rollup теперь должен жить не только в dashboard derivation-логике, но и как
machine-readable object `continuity_boundary_rollup` внутри `client_limit_meter_alignment`:
- `model_version = client-limit-continuity-boundary-rollup-v1`
- `state`
- `components`
- `observed_tokens`
- `observed_live_events`
- `note`

`component_event_coverage` теперь обязан быть `target-aware`, а не делить каждый
whole-cycle компонент на одинаковое число всех live events.
Для каждого компонента публикуются:
- `target_live_events_count`
- `target_scope_kind`
- `target_scope_applicable`

Это нужно затем, чтобы:
- `client_prompt` считался на своём real live scope;
- `assistant_generation` и `tool_overhead_outside_retrieval` считались только по
  `retrieval_lower_bound` live events;
- `continuity_restore_outside_retrieval` не считался missing там, где в scope не было
  `continuity_restore` event вообще.

`assistant_generation_observation_source` обязан показывать не просто общий blocker
`assistant_generation_unmeasured`, а конкретный source-gap:
- `assistant_generation_source_unavailable`
- `assistant_generation_source_no_scope_overlap`
- `assistant_generation_source_partial_scope_overlap`
- `assistant_generation_source_covers_missing_scope`

Это нужно затем, чтобы оператор мог отличить:
- отсутствие usable source вообще;
- отсутствие пересечения между usable source и текущим live scope;
- частичное покрытие missing scope;
- достаточное покрытие при всё ещё неcomplete same-meter path.

Начиная с этого же слоя same-meter contour умеет различать и тип usable source:
- `direct_turn_attach_v1`
- `codex_rollout_turn_timeline_v1`
- `direct_turn_attach_plus_rollout_turn_timeline_v1`

Это нужно затем, чтобы `assistant_generation` можно было materialize-ить не только
через rollout-derived matching, но и прямым turn-scoped attach без дублирования
тех же токенов по каждому `context_pack_id`.

Начиная с этого же слоя whole-cycle evidence можно подавать и в runtime path:
- `ContextPackArgs` / CLI context-pack умеют принимать optional observed fields:
  - `client_prompt_tokens`
  - `assistant_generation_tokens`
  - `tool_overhead_tokens`
  - `continuity_restore_tokens`
- те же поля теперь подняты и в front-door surfaces:
  - MCP `amai_context_pack`
  - MCP `amai_token_benchmark`
  - compatibility `memory search`
- отдельно materialized self-observed path:
  - `continuity startup` может сам записывать `continuity_restore_tokens` по длине
    собственного `CHAT_START_RESTORE` prompt-text;
  - для proof/verify запусков source kind обязан переводиться через
    `--token-source-kind proof_* / verify_*`, чтобы не contaminate live lane;
- отдельно materialized MCP tool-overhead path:
  - `amai_context_pack` после построения tool result связывает ответ с тем же
    `context_pack_id` и дописывает observed `tool_overhead_tokens` в исходное usage event;
  - для счёта берётся только MCP envelope поверх `context_pack_summary + stats`,
    а не полный retrieval payload;
  - это нужно затем, чтобы `tool_overhead_outside_retrieval` начинал честно
    materialize-иться без двойного счёта retrieval tokens;
  - тот же MCP path теперь тоже принимает `token_source_kind`, чтобы engineering traffic
    можно было отделять от live usage тем же truth guardrail;
- отдельно materialized CLI tool-overhead path:
  - `context pack` front door после записи usage event берёт реально сериализованный stdout
    JSON payload и считает observed CLI output overhead для того же `context_pack_id`;
  - proof:
    `scripts/proof_token_cli_tool_overhead.sh`;
  - это нужно затем, чтобы same-meter coverage не зависел только от MCP path и не терял
    фронтовой overhead у прямого `amai context pack`;
- отдельно materialized report-time tool-overhead auto-sync path:
  - если live retrieval event уже записан, но `tool_overhead_tokens` в нём ещё отсутствуют,
    `token-report` теперь может взять stored `context_pack` payload из registry по
    `context_pack_id`, пересчитать CLI-equivalent output overhead и дописать observed
    `tool_overhead_tokens` прямо в missing current scope;
  - это намеренно scoped only:
    auto-sync не бегает по всему lifetime, а работает только по active live scope
    (`current_session / rolling_window`), чтобы same-meter repair не превращался в
    бесконтрольный background rewrite;
  - proof:
    `scripts/proof_token_report_tool_overhead_autosync.sh`;
  - это важно затем, чтобы `tool_overhead_outside_retrieval` не оставался last-mile gap
    только потому, что событие было записано до materialization этого слоя;
- отдельно materialized post-call assistant-generation path:
  - `assistant_generation_tokens` нельзя честно требовать только pre-call, потому что
    upstream client узнаёт их уже после собственного ответа;
  - поэтому для того же `context_pack_id` теперь есть attach path:
    - CLI `observe token-whole-cycle-attach`
    - MCP tool `amai_observe_whole_cycle`
  - отдельно materialized и turn-group attach path:
    - CLI `observe token-whole-cycle-turn-attach --thread-id ... --turn-id ...`
    - MCP `amai_observe_whole_cycle_turn`
  - turn-group path считает `assistant_generation` один раз на весь `thread_id + turn_id`,
    а не размножает те же токены по каждому retrieval event;
  - в MCP `thread_id` можно не передавать только если `working_state` для всех
    `context_pack_ids` даёт ровно один thread; при неоднозначности inference fail-closed;
  - attach разрешён только fail-closed:
    первый attach допустим,
    повтор того же самого значения допустим,
    конфликтный overwrite другим числом запрещён;
  - это нужно затем, чтобы last missing same-meter component не оставался навсегда
    неobserved только из-за timing mismatch между tool call и финальным ответом клиента;
- дополнительно materialized rollout-backed assistant-generation path:
  - если runtime уже сохранил raw Codex rollout JSONL, `Amai` умеет извлечь оттуда
    post-call observed `assistant_generation_tokens` по `turn_id`;
  - CLI:
    `observe token-rollout-assistant-generation --rollout-path ... --repo-root ... --apply`
  - proof:
    `scripts/proof_token_rollout_assistant_generation.sh`
  - candidate принимается только fail-closed:
    в выбранном turn должен быть ровно один `context_pack_id`,
    а observed output tokens должны быть ненулевыми;
  - ambiguous rollout не имеет права silently привязывать assistant-generation к
    произвольному usage event;
  - report path теперь использует этот source не только вручную:
    при построении `token-report` он собирает все unambiguous rollout observations,
    затем пересекает их с live retrieval events, у которых `assistant_generation` ещё
    отсутствует, и только потом делает attach;
  - кроме этого same-meter layer теперь умеет честно поднимать turn-scoped coverage:
    по `working_state_event` он берёт `thread_id + captured_at_epoch_ms`, ищет подходящий
    rollout turn по времени и materialize-ит `assistant_generation` один раз на matched
    turn-group, если несколько `context_pack_id` попали в тот же самый ответ клиента;
  - shell heredoc/handoff/debug text с простым mention `context pack` теперь больше не
    имеет права делать turn `approved`: rollout source разрешён только для реального
    invocation path (`mcp__amai__amai_context_pack`, `cargo run ... context pack`,
    `./target/release/amai context pack`, `$AMAI context pack`, `memory search`);
  - это не даёт silently размножать одни и те же output tokens по каждому retrieval event,
    но и не теряет truthful partial coverage там, где один assistant turn обслужил сразу
    несколько retrieval context packs;
  - это уменьшает шанс, что report случайно поднимет only-lifetime evidence из старого
    turn вместо действительно missing current-scope events;
  - но same-meter claim всё равно запрещён, если usable rollout observations вообще не
    пересекаются с correlation set текущего `current_session / rolling_window` scope:
    в таком случае `assistant_generation` там обязан честно остаться `unmeasured`;
- это означает, что live caller может materialize-ить same-meter evidence не только через
  прямой бинарь `amai`, но и через MCP/bridge path;
- retrieval path не притворяется, что знает их сам;
- но если upstream client уже знает эти числа честно, он может materialize-ить их прямо в
  token ledger без repair/backfill обходов.

## Idempotency, backfill и corrections

Текущий billing-grade truth guardrail требует явно публиковать не только savings, но и
правила обращения с usage event.

Сейчас machine-readable contract уже должен показывать:
- `dedup_contract_version`
- `backfill_policy_version`
- `correction_policy_version`
- `event_time_policy_version`

Честный смысл этих правил сейчас такой:
- dedup key считается как `source_kind:event_id`;
- rollup-окна считаются по `occurred_at_epoch_ms`, а ingest time хранится отдельно;
- backfill пока разрешён только через явные `repair/reverify` paths;
- corrections пока остаются `report-only mutable snapshot`, а не invoice-grade credit flow.

То есть billing-grade governance ещё не завершён, но truth-contract уже не скрыт внутри
кода.

## Baseline fairness, billing policy и rate card

Следующий честный слой после usage-event contract — не деньги сами по себе, а правила,
по которым эти деньги когда-нибудь вообще можно будет считать.

Поэтому report теперь должен публиковать отдельно:
- `baseline_contract`
- `billing_policy`
- `rate_card`

`baseline_contract` нужен затем, чтобы:
- явно перечислить разрешённые baseline classes;
- явно перечислить запрещённые раздутые baseline classes;
- не дать savings расти за счёт нечестного baseline.

`billing_policy` нужен затем, чтобы:
- прямо показывать текущий mode;
- прямо показывать, что billable layer сейчас ещё отключён;
- не смешивать measured/report-only semantics с будущим денежным settlement;
- отдельно фиксировать truth-термины:
  - `savings floor`
  - `confirmed lower bound`
  - `retrieval savings floor`
  - `partial whole-agent-cycle lower bound`

Отдельно report теперь должен публиковать и `suitability_contract`.

Он нужен затем, чтобы:
- не путать отрицательный truthful KPI с непригодностью для review или billing;
- явно разводить surface-ы:
  - `operational_live`
  - `product_kpi`
  - `customer_review`
  - `contractual_export`
  - `billing_amount`
  - `compensation_pricing`;
- заставлять любой KPI читаться вместе с `coverage` и `completeness state`, а не как
  будто это уже полный settlement verdict.

`rate_card` нужен затем, чтобы:
- не делать вид, что токены уже переведены в деньги;
- явно фиксировать не только `currency_profile`, но и versioned binding layer;
- отличать честные состояния:
  - `not_configured`
  - `read_error`
  - `parse_error`
  - `bound_but_unpriced`
  - `priced_bound`

Практическое правило здесь такое:
- `money_conversion_enabled = true` разрешён только при `priced_bound`;
- если файл не подключён, не читается, не парсится или остаётся без цен, report
  обязан оставаться небиллинговым и денежные поля должны быть `null`.

Практическое правило сейчас такое:
- metering уже сильный;
- fairness/policy contract уже machine-readable;
- pricing и settlement пока ещё не materialized как money-facing layer.

## Settlement preview, freeze/close и late arrivals

До invoice-grade tokenonomics нельзя перескакивать через settlement semantics.

Поэтому report теперь должен публиковать:
- `settlement_contract`
- `statement_previews`

`settlement_contract` обязан честно отвечать на вопросы:
- какая версия statement preview сейчас действует;
- какая freeze/close policy используется;
- как трактуются late arrivals;
- какой correction/dispute policy сейчас materialized;
- закрыт ли уже реальный денежный workflow или это всё ещё report-only preview.

Текущий truthful status:
- `statement_version = settlement-preview-v6`
- `settlement_lifecycle_model_version = settlement-lifecycle-v4`
- `statement_period_governance_version = statement-period-governance-v2`
- `adjustment_preview_model_version = adjustment-preview-v1`
- `freeze_close_status = provisional_report_only`
- `late_arrival_status = deadline_from_latest_event_report_only`
- `current_operational_state = live_measurement_open`
- `current_contractual_state = report_only_preview_open`
- `current_materialized_boundary = measured_report_only`
- `materialized_settlement_stages` публикуют только реально materialized report-only стадии
- `future_reserved_settlement_stages` отдельно перечисляют будущие billable/settled/invoiced/credited/disputed/closed стадии
- corrections/disputes пока не invoice-grade, а только report-only semantics

`statement_previews` нужны затем, чтобы по каждому scope показать:
- measured non-billable lower bound;
- coverage;
- settlement status;
- settlement stage;
- next settlement stage candidate;
- lifecycle state;
- contractual state;
- close barriers;
- period window;
- adjustment preview;
- internal observed whole-cycle lower bound без дублирования turn-scoped `assistant_generation`
  по каждому retrieval event, если output materialized только на уровне `thread_id + turn_id`;
- и при этом не подсовывать пользователю фальшивую сумму к оплате.

Именно поэтому в текущем runtime:
- `billable_lower_bound_tokens = null`
- `final_amount = null`
- `statement_status = report_only_preview`
- `lifecycle_state = measured_non_billable_open`
- `contractual_state = report_only_preview_open`
- `settlement_stage` уже может быть:
  - `empty_report_only`
  - `measured_open_report_only`
  - `measured_review_ready_report_only`
  - `measured_adjusted_report_only`
  - `measured_pending_adjustment_report_only`
  - `measured_disputed_report_only`
- `next_settlement_stage_candidate` честно отделяет:
  - `awaiting_measured_usage`
  - `review_ready_blocked`
  - `billable_blocked`
  - `billable_reserved`
- `transactional_statuses` теперь отдельно раскладывают:
  - `measured`
  - `review`
  - `billable`
  - `settled`
  - `invoiced`
  - `credited`
  - `disputed`
  - `closed`
  и для каждого фиксируют `boundary`, `materialized` и `blocking_reasons`
- `provisional_close_state` уже может быть:
  - `report_only_preview_provisionally_stable`
  - `report_only_preview_provisional_hold`
- `provisional_close_candidate` показывает, можно ли уже считать scope устойчивым в report-only
  смысле, даже если billing ещё не materialized
- `close_barriers` прямо перечисляют, почему period нельзя закрыть честно
- `period.close_at_epoch_ms = null`, пока честного close workflow ещё нет
- `period.provisional_close_earliest_at_epoch_ms` и `period.late_arrival_deadline_epoch_ms`
  уже публикуются и считаются от `latest_event + late_arrival_grace`
- `adjustment_preview.status = default_path_missing`, пока repo-local registry файл ещё
  не materialized

Это не недостаток UX, а truth guardrail до тех пор, пока реальный billing workflow не
materialized end-to-end.

Теперь поверх preview/report layer ещё живёт и `suitability`:
- для `product_kpi`
- для `customer_review`
- для `contractual_export`
- для `billing_amount`
- для `compensation_pricing`

Главный смысл этого слоя:
- suitability не говорит, хорошая цифра или плохая;
- suitability говорит, где эту цифру уже можно использовать без подмены смысла;
- отрицательная экономия тоже может быть truthful product KPI, если confirmed lower bound
  уже есть и рядом опубликованы coverage/completeness.

## Adjustment schema и report-only registry

После period governance следующий честный слой — не “исправим потом как удобно”, а
отдельные `adjustment_request_schema` и `adjustment_registry`.

Зачем они нужны:
- corrections/disputes должны жить отдельными entries;
- прошлый period нельзя quietly переписывать задним числом;
- customer-facing audit должен видеть, есть ли вообще pending/applied/disputed corrections.

Теперь report отдельно публикует:
- `adjustment_request_schema`
- `adjustment_registry`

`adjustment_request_schema` обязан честно отвечать на вопросы:
- какие поля обязательны для future correction/credit/dispute entry;
- какие `kind` и `status` разрешены;
- можно ли делать ретро-перезапись старого statement.

Текущий truthful status:
- `adjustment_request_schema_version = adjustment-request-v1`
- `retroactive_rewrite_policy = forbidden_use_adjustment_entries`

`adjustment_registry` обязан честно отвечать на вопросы:
- есть ли вообще source registry;
- сколько entries уже есть;
- сколько из них pending/applied/disputed;
- какой у них per-scope hash.

Текущий truthful status без подключённого источника:
- `adjustment_registry_version = adjustment-registry-v2`
- `resolved_path = /home/art/agent-memory-index/state/token_adjustment_registry.json`
- `status = default_path_missing`
- `entries_count = 0`
- `registry_hash = null`

Operator-safe report-only команды:

```bash
./target/release/amai observe token-adjustment-registry --scope lifetime
./target/release/amai observe token-adjustment-add \
  --scope lifetime \
  --kind adjustment_entry \
  --status pending_review \
  --reason-code contaminated_live_session
./target/release/amai observe token-adjustment-add \
  --scope lifetime \
  --status pending_review \
  --reason-code contaminated_live_session \
  --resolve-related-statement-id
```

Смысл этих команд:
- registry можно materialize-ить без денежного settlement;
- entries живут отдельным слоем, а не тихой перезаписью старого report;
- `applied_report_only` влияет на preview как adjustment ledger, но не превращает его в invoice.
- `--resolve-related-statement-id` нужен затем, чтобы correction entry честно ссылался на
  актуальный `statement_preview_id` без ручного копирования из другого отчёта.

Именно поэтому `adjustment_preview` внутри `statement_previews` теперь читает registry-слой,
а не рисует credits “по ощущениям”.

## Contractual vs operational surfaces

Сильный measuring engine ещё не даёт права смешивать инженерную телеметрию и contractual
метрики для клиента.

Поэтому report теперь должен публиковать отдельный `telemetry_surfaces`.

Его смысл такой:
- `operational_surface`
  - live dashboard и observability для инженеров;
- `contractual_surface`
  - report-only tokenonomics contract для review, audit и будущего settlement.

Текущий truthful status:
- `telemetry_surface_split_version = tokenonomics-surface-split-v1`
- dashboard headline и live rollups нельзя трактовать как invoice;
- contractual export должен идти через `statement_previews`, `reconciliation_previews`,
  `margin_view` и `contractual_evidence_pack`, а не через operational live-card.

## Provider reconciliation и внешний truth source

После settlement-preview следующий честный слой — не “сразу деньги”, а явный
`reconciliation_contract`.

Зачем он нужен:
- не делать вид, что внутренний lower bound уже сверен с provider usage;
- не терять разницу между `internal measured truth` и `external billing truth`;
- не скрывать, каких файлов и каких policy слоёв ещё не хватает до money-grade режима.

Теперь report отдельно публикует:
- `reconciliation_contract`
- `reconciliation_previews`

`reconciliation_contract` обязан честно отвечать на вопросы:
- какие внутренние truth layers уже есть;
- какие внешние sources нужны для сверки;
- какие из них обязательны;
- готовы ли мы вообще к external reconciliation.

Текущий truthful status:
- `reconciliation_contract_version = provider-reconciliation-v10`
- `ready_for_external_reconciliation` теперь зависит от реального bind provider usage export,
  а не от одного факта, что где-то прописан путь;
- contract теперь ещё отдельно публикует:
  - `usage_truth_completeness_state`
  - `rate_card_truth_completeness_state`
  - `provider_cost_truth_completeness_state`
  - `invoice_evidence_completeness_state`
  - `money_truth_completeness_state`
  - `reconciliation_readiness_state`
  - `governance_blocking_reasons`
  - `source_requirements.required_sources_for_usage_truth`
  - `source_requirements.required_sources_for_cost_truth`
  - `source_requirements.optional_sources_for_invoice_evidence`
  - `source_requirements.unready_*`
- `external_truth_sources` теперь на верхнем уровне report тоже несёт:
  - `provider_usage_export`
  - `provider_invoice_export`
  - `provider_rate_card`
  - `infra_cost_profile`
  вместе с `truth_roles`, чтобы было видно не только где лежит файл, но и для какого слоя правды
  он вообще нужен;
- scope-level preview теперь ещё отдельно публикует temporal truth:
  - `provider_usage_scope_alignment_state`
  - `provider_invoice_scope_alignment_state`
  - `rate_card_scope_alignment_state`
  - `temporal_truth_state`
- scope-level preview теперь ещё отдельно публикует provider-identity truth:
  - `rate_card_provider_alignment_state`
  - `invoice_provider_alignment_state`
  - `provider_identity_state`
- эти поля нужны затем, чтобы external truth не считалась честно применимой к scope только
  потому, что файл уже привязан и арифметика сошлась;
  - теперь ещё отдельно видно, покрывают ли provider usage, invoice export и rate card
    именно период текущего statement preview;
- внутренний lower bound уже materialized;
- provider sources теперь могут materialize-иться не только через env, но и через repo-local
  default files:
  - `state/provider_usage_export.json`
  - `state/provider_invoice_export.json`
  - `state/provider_rate_card.json`
  - `state/infra_cost_profile.json`
- если env-binding не задан и repo-local file ещё не materialized, truthful status теперь
  `default_path_missing`;
- при честном runtime bind report теперь отдельно показывает:
  - `provider_usage_binding`
  - `provider_invoice_binding`
  - и те же bindings внутри `reconciliation_contract.external_truth_bindings`.

Operator-safe inspect path:

```bash
./target/release/amai observe token-contractual-sources --scope lifetime
```

Эта команда нужна затем, чтобы без парсинга всего token report сразу увидеть:
- source bindings;
- reconciliation preview;
- margin scope;
- statement export preview.

Customer-facing export bundle:

```bash
./target/release/amai observe token-statement-export \
  --scope lifetime \
  --output-dir /tmp/amai-token-statement
```

Этот bundle materialize-ит отдельные файлы:
- `manifest.json`
- `settlement_report_preview.json`
- `statement_export_preview.json`
- `contractual_evidence_pack.json`
- `token_contractual_sources.json`

Смысл bundle:
- дать stable review/export surface;
- не заставлять клиента или инженера собирать statement/evidence/source bindings вручную;
- сохранить truth guardrail: bundle остаётся `report_only`, даже если hashes и previews уже
  готовы.

Именно поэтому truthful `reconciliation_previews` теперь обязаны выглядеть так:
- `internal_measured_non_billable_lower_bound_tokens` остаётся про lower bound savings;
- `internal_provider_billed_tokens` отдельно показывает внутреннюю observed whole-cycle
  lower bound для того же usage-мера;
- `internal_observed_whole_cycle_lower_bound_tokens` и
  `verified_internal_observed_whole_cycle_lower_bound_tokens` делают этот переход явным и не
  дают перепутать retrieval-only usage с уже materialized same-meter компонентами;
- отдельный governance-layer теперь показывает:
  - есть ли уже `usage truth`
  - есть ли уже `money truth`
  - дошли ли мы только до usage-bind, до usage+cost truth или уже до invoice-side evidence
- `drift_tokens` считается только как
  `internal_provider_billed_tokens - external_provider_usage_tokens`;
  - `external_provider_usage_tokens`, `external_provider_cost_amount` и
    `external_invoice_amount` могут заполняться только после реального bind соответствующих
    external sources;
  - при priced rate card теперь может materialize-иться и
    `internal_provider_cost_estimate_amount`;
  - `drift_amount` теперь честно показывает money-side difference между внутренней
    input-side cost estimate и external provider cost;
  - `invoice_drift_amount` отдельно показывает drift между provider usage cost и invoice export.

Это не пробел в арифметике, а truth guardrail:
- пока внешний источник не подключён;
- пока rate card не materialized;
- пока reconciliation parser не привязан к canonical ledger;

`Amai` не имеет права сравнивать provider usage с savings и выдавать это за честный drift.

## Margin view и собственная экономика Amai

После reconciliation следующий честный слой — `margin_contract` и `margin_view`.

Зачем они нужны:
- не путать `customer savings` и `product margin`;
- не выдавать токеновую экономию клиента за денежную прибыль `Amai`;
- явно показывать, когда у нас ещё нет rate card или infra cost profile.

Теперь report отдельно публикует:
- `margin_contract`
- `margin_view`

`margin_contract` обязан честно отвечать на вопросы:
- какой versioned margin model сейчас действует;
- есть ли вообще priced rate card;
- есть ли infra cost profile;
- включена ли money-margin арифметика.

Текущий truthful status:
- `margin_model_version = margin-view-v9`
- `infra_cost_binding_model_version = infra-cost-binding-v3`
- `infra_cost_profile_version = unpriced-infra-v1`
- `money_margin_enabled` включается только после честного bind на
  `priced rate card + provider usage + infra cost profile`;
- margin layer теперь отдельно раскладывает:
  - `customer_savings_money_truth_completeness_state`
  - `amai_cost_truth_completeness_state`
  - `margin_truth_completeness_state`
  чтобы было видно не только готов ли весь preview, но и готова ли уже денежная нижняя граница
  savings для клиента и готова ли оценка собственного infra cost;
- truthful aligned-state для reconciliation теперь тоже отдельный:
  - `external_usage_aligned_report_only`
  - `external_usage_and_invoice_aligned_report_only`
- `margin_view` теперь обязан брать priced/unpriced не из static contract label, а из
  настоящего rate-card binding runtime.
- `margin_view` теперь ещё отдельно публикует:
  - `rate_card_truth_completeness_state`
  - `infra_cost_truth_completeness_state`
  - `pricing_truth_completeness_state`
  - `required_sources_for_margin_truth`
  - `unready_required_sources_for_margin_truth`
  - `margin_confidence_state`
  - `margin_readiness_state`
  - `rate_card_scope_alignment_state`
  - `infra_cost_scope_alignment_state`
  - `temporal_truth_state`
  - `provider_identity_state`

Новый смысл этих полей:
- `rate_card_truth_completeness_state`
  - показывает, дошли ли мы хотя бы до честно привязанного pricing source;
- `infra_cost_truth_completeness_state`
  - показывает, есть ли уже отдельный truthful источник собственных infra costs;
- `pricing_truth_completeness_state`
  - показывает итог по pricing-layer целиком, не смешивая его с usage truth;
- `margin_readiness_state`
  - нормализует, какой именно следующий реальный блокер сейчас мешает money-preview:
    - `awaiting_pricing_truth`
    - `awaiting_usage_truth`
    - `provider_identity_mismatch`
    - `pricing_period_mismatch`
    - `currency_profile_mismatch`
    - `provider_drift_detected`
    - `temporal_truth_unscoped_report_only`
    - `preview_ready_report_only`

Именно поэтому truthful `margin_view` теперь обязан выглядеть в одной из двух форм:
- до bind внешних truth-sources:
- `customer_saved_tokens_lower_bound` заполнен;
- `customer_saved_amount_lower_bound = null`
- `amai_infra_cost_amount = null`
- `margin_amount = null`
- `savings_to_cost_ratio = null`
- после честного bind `provider usage + rate card + infra cost profile`:
  - `customer_saved_amount_lower_bound` может быть заполнен;
  - `amai_infra_cost_amount` может быть заполнен;
  - `margin_amount` может быть заполнен;
  - `savings_to_cost_ratio` может быть заполнен;
  - но state всё равно остаётся `report_only preview`, а не invoice;
  - если period truth не подтверждён, state обязан стать:
    - `priced_preview_temporal_unscoped_report_only`
    - или `pricing_period_mismatch`
    а не делать вид, что pricing уже финально пригоден к scope.
  - если provider usage и priced rate card смотрят на разные provider identities, state обязан стать:
    - `provider_identity_mismatch`
    а не продолжать money preview как будто provider truth уже согласован.

Это не “недоделанная формула”, а truth guardrail:
- пока rate card остаётся `unpriced`;
- пока infra cost profile не materialized;
- пока provider reconciliation не привязан хотя бы к usage export;

`Amai` не имеет права рисовать даже приблизительную маржу как будто она уже доказана.

## Contractual evidence pack

Следующий честный слой после `statement_preview + reconciliation_preview + margin_view` —
это не invoice, а отдельный `contractual_evidence_pack`.

Но поверх него теперь уже нужен и короткий `contractual_statement_summary`.

Зачем он нужен:
- не заставлять клиента и sales читать весь evidence pack ради одного статуса;
- показывать короткий truthful state по scope:
  - `contractual_state`
  - `coverage_state`
  - `reconciliation_state`
  - `margin_state`
  - `blocking_reasons`
- оставлять evidence pack как следующий слой доказательств, а не как единственный UI-формат.

Зачем он нужен:
- отдать customer-facing evidence/export одним JSON-пакетом;
- зафиксировать состав included/excluded usage line items;
- не заставлять клиента читать dashboard вместо audit-friendly пакета;
- не подмешивать сырой текст запроса туда, где нужен только contract-level след.

Теперь этот export должен собираться отдельной командой:
- `observe token-evidence-pack --scope current_session`
- `observe token-evidence-pack --scope rolling_window`
- `observe token-evidence-pack --scope lifetime`

Внутри pack обязаны быть:
- `pack_version`
- `scope_code`
- `scope_label`
- `truth_guardrail`
- `contract_versions`
- `statement_preview`
- `reconciliation_preview`
- `margin_scope`
- `included_events_count`
- `excluded_events_count`
- `included_events_hash`
- `excluded_events_hash`
- `line_items.included`
- `line_items.excluded`

Честный смысл этого export сейчас такой:
- это `contractual-evidence-pack-v13`;
- это всё ещё `report_only tokenonomics`;
- это не invoice;
- это не final settlement;
- это не разрешение quietly подменять прошлый period.

Поверх export теперь отдельно materialize-ится и `external_truth_manifest`.

Он нужен затем, чтобы customer-facing review surface видела не только сами statement/reconciliation
состояния, но и audit-friendly fingerprint привязанных truth sources:
- `provider_usage_export`
- `provider_invoice_export`
- `provider_rate_card`
- `infra_cost_profile`
- `token_adjustment_registry`

У каждого source в manifest теперь должны быть:
- `status`
- `binding_status`
- `resolved_path`
- `source_bytes`
- `source_sha256`
- `source_last_modified_epoch_ms`
- `schema_version`
- `bound_version`
- `provider`
- `currency_profile`

Именно поэтому contractual review/export теперь обязан не просто ссылаться на внешние truth
sources “словами”, а показывать machine-readable fingerprint того, что реально было привязано в
момент сборки export.

Отдельный truth guardrail внутри pack обязателен:
- `retrieval_savings_floor = real`
- `partial_whole_agent_cycle_lower_bound = real`
- `full_session_economics = not_fully_measured`

Отдельное правило redaction:
- raw `query` в pack не попадает;
- остаются только `query_hash`, scope, usage-state и token arithmetic;
- это нужно затем, чтобы audit/export слой не тащил лишний customer content.

Hashes по line items нужны затем, чтобы:
- доказать состав export без ручного перебора;
- не подменять included/excluded состав незаметно;
- иметь audit-friendly anchor для будущих settlement/dispute flows.

Поверх полного pack теперь нужен и отдельный:
- `statement_export_previews.current_session / rolling_window / lifetime`
- `settlement_report_previews.current_session / rolling_window / lifetime`

У `statement_export_preview` теперь ещё отдельно публикуются pricing-source поля:
- `rate_card_status`
- `rate_card_version`
- `rate_card_provider`
- `rate_card_currency_profile`
- `provider_usage_provider`
- `provider_invoice_provider`
- `margin_blocking_reasons`

Это не дублирование, а отдельный слой:
- preview даёт компактный `statement_preview_id`;
- preview теперь ещё несёт готовый `settlement_report_preview`, чтобы review/export не
  собирал period anchors, hashes и policy snapshot вручную;
- preview теперь ещё несёт `settlement_stage`, `settlement_stage_family` и `next_settlement_stage_candidate`;
- preview теперь ещё отдельно несёт readiness-axis:
  - `internal_money_arithmetic_readiness_state`
  - `internal_money_arithmetic_blocking_reasons`
  - `contractual_settlement_readiness_state`
  - `contractual_settlement_blocking_reasons`
  Это нужно затем, чтобы export не смешивал:
  - внутреннюю готовность money-arithmetic preview;
  - и более строгую contractual/settlement readiness;
- preview, settlement report preview и evidence pack теперь ещё несут
  `customer_contractual_boundary`;
- boundary отдельно фиксирует:
  - `review_surface_state`
  - `review_surface_blocking_reasons`
  - `future_settlement_activation_state`
  - `future_settlement_activation_blocking_reasons`
  Это нужно затем, чтобы customer-facing review layer и будущий settlement activation
  были видны как два разных договора, а не как одна обобщённая готовность.
- preview, settlement report preview, contractual sources и evidence pack теперь ещё несут
  `settlement_activation_governance`;
- governance отдельно фиксирует:
  - `governance_state`
  - `next_settlement_stage_candidate`
  - `next_settlement_stage_blockers`
  - `provisional_close_barriers`
  - `billing_close_barriers`
  - `close_barriers`
  - `registry_status / adjustment_status`
  - `credit_action_state / dispute_action_state`
  Это нужно затем, чтобы report-only review surface явно показывал, какие governance и
  adjustment-барьеры удерживают будущую settlement activation.
- preview теперь ещё несёт `transactional_statuses`, чтобы export не путал уже materialized
  measured/report-only стадии с будущими reserved billing стадиями;
- preview и evidence pack теперь ещё несут `export_semantics`, чтобы customer review surface
  не смешивался с operational telemetry и не выглядел как invoice-grade settlement;
- statement preview и `agent_cycle_economics` теперь ещё несут
  `client_limit_meter_alignment`;
- этот слой отдельно фиксирует:
  - `alignment_state`
  - `same_meter_as_client_limit`
  - `live_events_count / non_live_events_count`
  - `measured_components / partially_measured_components / missing_components`
  - `component_event_coverage`
  - `blocking_reasons`
  Это нужно затем, чтобы live lower-bound savings не притворялись уже эквивалентными
  тому же самому полному метру, которым внешний клиент считает общий `5h` limit.
- dashboard hero-cards обязаны поднимать этот same layer в user-facing виде:
  - отдельная строка `Связь с лимитом клиента`;
  - честное различение `only_non_live_scope_activity`,
    `live_usage_unconfirmed_not_meter_equivalent` и
    `partial_lower_bound_not_meter_equivalent`,
    `whole_cycle_partially_observed_not_meter_equivalent`,
    `whole_cycle_observed_baseline_partial`,
    `whole_cycle_observed_explicit_boundary_not_meter_equivalent`;
  - note карточки обязан прямо сказать, почему её число не обязано совпадать
    с внешней клиентской шкалой лимита.
- публикует `included_events_hash / excluded_events_hash`;
- отдельно показывает `credit_action_state` и `dispute_action_state`;
- и хранит рядом уже готовые `statement_preview / reconciliation_preview / margin_scope`;
- даёт готовую команду `observe token-evidence-pack` для полного export.

Именно так customer-facing export должен масштабироваться:
- сначала compact preview;
- затем on-demand full evidence pack;
- и только потом, в будущем, settlement/dispute workflow.

Текущие surface versions для этого слоя:
- `contractual-statement-export-v20`
- `settlement-report-preview-v11`
- `contractual-evidence-pack-v20`
- `client-limit-meter-alignment-v9`
- `client-limit-baseline-equivalence-v3`
- `client-limit-strict-meter-slice-v1`
- `client-limit-explicit-boundary-surface-v1`
- `client-limit-continuity-boundary-rollup-v1`
- `adjustment-activation-governance-v1`

Начиная с этих версий customer-facing export/report surface больше не держит
`continuity boundary` только внутри dashboard:
- `statement_export_preview`
- `settlement_report_preview`
- `contractual_evidence_pack`

теперь отдельно несут compact `client_limit_boundary_semantics`, где есть:
- `strict_client_meter_slice`
- `explicit_boundary_surface`
- `continuity_boundary_rollup`

Это нужно затем, чтобы customer/audit preview видел честную границу между:
- measured strict same-meter lower bound;
- и Amai-specific continuity boundary вне strict client-meter slice.

Теперь те же customer-facing surface-ы ещё несут `adjustment_activation_governance`.
Это отдельный report-only governance слой, который показывает:
- готов ли future adjustment path;
- чем он заблокирован;
- и какие correction/credit/dispute semantics уже materialized в preview.

## Preliminary vs stable

Пока выборка маленькая, headline нельзя подавать как устойчивый итог.

Рекомендуемый минимум:
- `events_count >= 50`
- или `total_baseline_tokens >= 100000`

До этого threshold метрика должна быть помечена как:
- `preliminary`

## Metering freshness и contractual lag

Billing-grade tokenonomics не может жить только на суммах savings.
Нужно отдельно публиковать:
- насколько жив сам metering pipeline;
- и закрыто ли уже окно поздних событий.

Для этого в report теперь есть отдельный слой:
- `metering_freshness_contract`
- `metering_freshness.current_session / rolling_window / lifetime`

Обязательные поля этого слоя:
- `metering_ingest_state`
  - `empty`
  - `within_slo`
  - `soft_lag`
  - `lagging`
- `contractual_lag_state`
  - `empty`
  - `awaiting_late_events`
  - `lag_window_elapsed`
- `contractual_freshness_state`
  - `empty`
  - `provisional_open_window`
  - `stable`
  - `lagging_pipeline`
- `latest_event_age_ms`
- `latest_ingest_lag_ms`
- `p95_ingest_lag_ms`

Это нужно затем, чтобы не путать две разные проблемы:
- pipeline действительно опаздывает с ingest;
- или ingest уже нормальный, но окно late-arrival ещё честно открыто.

`statement_preview.close_barriers` теперь тоже обязан учитывать этот слой:
- `late_arrival_window_open`
- `metering_pipeline_lagging`

То есть customer-facing statement preview теперь не делает вид, что scope уже
стабилен, если:
- события ещё слишком свежие и в окно могут приехать поздние связки;
- или сам metering pipeline уже вышел за ingest SLO.

## Cold / warm разрез

Каждое событие должно нести `cold_warm_state`.

Нужные состояния:
- `cold`
- `warm`
- `post_restart`
- `post_reindex`
- `post_warmup`

В report должен быть отдельный разрез:
- `temperature_slices`

Он нужен затем, чтобы:
- warm-cache победы не маскировали первый тяжёлый запрос;
- cold-path можно было оценивать отдельно.

## Query slices

Ledger должен уметь срезы по реальному типу вопроса.

Рекомендуемые query slices:
- `code_lookup`
- `symbol_lookup`
- `docs_lookup`
- `architecture_question`
- `bugfix_context`
- `onboarding_query`
- `cross_file_trace`
- `config_lookup`

Для каждого такого slice нужно показывать:
- `events_count`
- `verified_effective_savings_pct`
- `verified_answer_like_savings_pct`
- `quality_ok_rate`
- `fallback_rate`
- `p95_latency_ms`

## Что должно быть на пользовательской панели

Пользовательская панель должна показывать:
- `Verified effective savings`
- `Saved tokens`
- `Quality-ok rate`
- `Fallback rate`
- `Answer-like rate`
- `Events counted`

Не нужно выносить в headline:
- лучший одиночный benchmark;
- максимальный single-event win;
- смешанный `live + proof` результат;
- tiny-sample проценты без пометки `preliminary`.

## Anti-inflation rules

Ledger считается trustworthy только если одновременно выполняется всё:

1. есть разделение `live` и `verify`;
2. baseline реалистичен;
3. aggregate savings считаются по сумме токенов, а не по среднему процентов;
4. recovery penalties реально вычитаются;
5. primary KPI gated by quality;
6. маленькая выборка помечается как `preliminary`;
7. audit-поля позволяют проверить расчёт задним числом.

## Что сейчас считается каноническим headline в Amai

Канонический headline metric:

`Verified Effective Savings %`

При этом в текущем runtime `Amai` уже отдельно считает и более строгий secondary contour:
- `verified_answer_like_savings_pct`
- `answer_like_rate`

Он нужен затем, чтобы не путать:
- широкий quality-gated verified KPI;
- и более строгую долю событий, где контекст уже выглядит достаточным для полезного ответа без лишнего recovery.

По-человечески это означает:
- это не “лучшая цифра в лаборатории”;
- это не “сырые savings без штрафов”;
- это живая проверенная экономия токенов на реальной работе.
`current_session` теперь обязан использовать этот session grouping буквально:
- сначала берётся latest `session_id`;
- все события с тем же `session_id` считаются текущей logical session;
- если у latest события `session_id` пустой, только тогда разрешён fallback к old time-gap heuristic;
- `live_continuity_startup` теперь считается канонической boundary для новой logical session.
- runtime proof:
  `scripts/proof_token_session_boundary.sh`
