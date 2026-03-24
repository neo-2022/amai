modified_at: 2026-03-24 11:31 MSK
Ручная сверка guide/docs: 2026-03-24 11:31 MSK

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
- не смешивать measured/report-only semantics с будущим денежным settlement.

`rate_card` нужен затем, чтобы:
- не делать вид, что токены уже переведены в деньги;
- явно фиксировать `unpriced` состояние до появления настоящего pricing profile.

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
- `statement_version = settlement-preview-v1`
- `freeze_close_status = not_enforced_report_only`
- `late_arrival_status = accepted_until_settlement_exists`
- corrections/disputes пока не invoice-grade, а только report-only semantics

`statement_previews` нужны затем, чтобы по каждому scope показать:
- measured non-billable lower bound;
- coverage;
- settlement status;
- и при этом не подсовывать пользователю фальшивую сумму к оплате.

Именно поэтому в текущем runtime:
- `billable_lower_bound_tokens = null`
- `final_amount = null`
- `statement_status = report_only_preview`

Это не недостаток UX, а truth guardrail до тех пор, пока реальный billing workflow не
materialized end-to-end.

## Preliminary vs stable

Пока выборка маленькая, headline нельзя подавать как устойчивый итог.

Рекомендуемый минимум:
- `events_count >= 50`
- или `total_baseline_tokens >= 100000`

До этого threshold метрика должна быть помечена как:
- `preliminary`

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
