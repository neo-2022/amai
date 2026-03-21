modified_at: 2026-03-21 03:48 MSK
Ручная сверка guide/docs: 2026-03-21 03:48 MSK

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
- `timestamp_utc`
- `session_id` или эквивалент session grouping
- `rolling_window_profile`
- `traffic_class`
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

Сильно желательные поля:
- `target_kind`
- `baseline_hit_target`
- `amai_hit_target`
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
- `quality_ok_rate`
- `fallback_rate`
- `p95_latency_ms`

## Что должно быть на пользовательской панели

Пользовательская панель должна показывать:
- `Verified effective savings`
- `Saved tokens`
- `Quality-ok rate`
- `Fallback rate`
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

По-человечески это означает:
- это не “лучшая цифра в лаборатории”;
- это не “сырые savings без штрафов”;
- это живая проверенная экономия токенов на реальной работе.
