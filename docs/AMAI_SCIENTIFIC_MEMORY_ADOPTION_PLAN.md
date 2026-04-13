# Scientific Memory Adoption Plan For Amai

Дата ручной сверки: 2026-04-13

Источники:
- `/home/art/Загрузки/AMAI_audit_and_improvement_plan.pdf`
- `/home/art/Загрузки/tv&ms_ff.pdf`

## Source trust model

Этот документ не делает вид, что оба PDF равны по весу с текущим репозиторием `Amai`.

Правило source-of-truth для этой сверки такое:
- текущий repo state, proof/gate surfaces и canonical docs главнее любого внешнего PDF;
- `AMAI_audit_and_improvement_plan.pdf` считается `project-snapshot / non-authoritative`;
- `tv&ms_ff.pdf` считается `methodological source / non-product-proof`;
- ни один repo-claim из первого PDF не принимается без ручной сверки с текущим деревом;
- ни одна математическая идея из второго PDF не считается автоматически product-ready без contour mapping и proof path.

Что было вручную использовано как текущая истина:
- `docs/AUDIT_MANUAL_VERDICT_2026-04-11.md`
- `docs/IMPLEMENTATION_STATUS.md`
- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
- `docs/IMPLEMENTATION_GATES.md`
- `docs/ARCHITECTURE.md`
- `sql/000_bootstrap.sql`
- `src/working_state.rs`
- `src/retrieval.rs`
- `src/postgres.rs`
- `src/continuity.rs`
- `./scripts/agent_preflight.sh --json`
- `./scripts/maintainability_gate.sh --json`
- `./scripts/amai_exec.sh benchmark coverage`

Нормализация PDF:
- `AMAI_audit_and_improvement_plan.pdf`
  - 13 страниц
  - `CreationDate`: 2026-04-12
  - явный self-disclosure внутри текста: аудит статический, `cargo`/runtime proof в той среде не запускались
- `tv&ms_ff.pdf`
  - 113 страниц
  - конспект по вероятностям, статистике, регрессии, цепям Маркова и процессу Пуассона

## Execution contract for another model

Этот документ теперь нужно читать не только как synthesis, но и как authoritative implementation-program.

Если его дают другой модели, она обязана:
- считать этот документ исполняемым playbook, а не просто обзором идей;
- не изобретать параллельный roadmap;
- не менять порядок очередей без явного обновления этого документа;
- не подменять отсутствующее решение собственной эвристикой;
- если в коде встречается развилка, не описанная здесь или в canonical docs проекта, сначала обновить этот документ, а не молча выбрать вариант.

Если этот документ конфликтует с:
- `AGENTS.md`;
- `docs/IMPLEMENTATION_GATES.md`;
- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`;
- `docs/MAINTAINABILITY_ENFORCEMENT.md`;
то они имеют приоритет, а implementer обязан fail-closed остановиться и сначала синхронизировать этот документ.

### Что этот документ должен обеспечить после полного исполнения

После полного исполнения этого документа в production должны быть materialized не "идеи на бумаге", а следующие рабочие contours:
- statistical benchmark honesty с CI / significance / drift surfaces;
- explainable lifecycle transition discipline как first-class runtime/reporting contour;
- `Markov / hazard lifecycle v1` как advisory/explain surface поверх Stage 9, без truth-authority;
- regression explain surface как read-only explanatory contour;
- Poisson/arrival capacity forecast как planning/observability contour, без runtime enforcement.

Этот документ не даёт права автоматически реализовывать:
- truth-affecting Bayesian belief-layer;
- destructive auto-decision из probabilistic score;
- замену `verified truth` математической проекцией;
- numeric posterior promotion без отдельного measured approval revision этого же документа.

Коротко:
- production scope этого документа = advisory/proof-grade probabilistic/statistical contours;
- out-of-scope = truth-authoritative probabilistic promotion.

### Production definition of done

Каждая очередь из execution program считается закрытой только если одновременно выполнены все условия:
- schema/code/CLI/observe/dashboard/docs обновлены там, где это требуется этой очередью;
- новые поля surfaced не только в коде, но и в machine-readable output;
- companion non-regression по `speed / accuracy / quality / truth` не сломан;
- stage-local proof bundle зелёный;
- нет silent fallback, где отсутствие данных маскируется как уверенный verdict;
- rollback/recovery path остаётся понятным;
- continuity handoff обновлён.

Что не считается завершением:
- только docs без code/runtime materialization;
- только raw math helper без CLI/observe/dashboard surface;
- только один локальный smoke;
- только красивый summary без raw-result lane.

### Canonical execution order

Ниже authoritative очередь внедрения.
Следующая очередь не стартует, пока предыдущая не доведена до passing state или не помечена в этом документе как blocked с exact root-cause и unblock condition.

#### Queue 0. Preflight and baseline freeze

Цель:
- зафиксировать baseline, чтобы probabilistic/statistical улучшения не ломали текущий product law.

Обязательные чтения перед кодом:
- `AGENTS.md`
- `docs/IMPLEMENTATION_STATUS.md`
- `docs/ARCHITECTURE.md`
- `docs/OPERATIONS.md`
- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
- `docs/IMPLEMENTATION_GATES.md`
- `docs/MAINTAINABILITY_ENFORCEMENT.md`

Обязательные baseline-команды:
- `./scripts/agent_preflight.sh --json`
- `./scripts/maintainability_gate.sh --json`
- `./scripts/amai_exec.sh benchmark coverage`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_forgetting_consolidation.sh`
- `./scripts/proof_observability.sh`

Что надо зафиксировать как baseline artifact set:
- benchmark coverage summary;
- memory task matrix summary;
- MCP task matrix summary;
- observability snapshot summary;
- forgetting/governance dashboard snapshot или raw equivalent.

Выход из Queue 0 разрешён только если:
- baseline либо зелёный, либо все уже существующие красные зоны локализованы как pre-existing и явно не созданы текущей очередью;
- implementer понимает, какие companion proofs обязательны для каждой следующей очереди.

#### Queue 1. Statistical benchmark honesty

Цель:
- убрать состояние, где benchmark contour говорит "лучше", но не показывает measured uncertainty и drift.

Точные code surfaces:
- `src/benchmark_matrix.rs`
- `src/memory_task_matrix.rs`
- `src/mcp_task_matrix.rs`
- `src/observe.rs`
- `src/mcp.rs`
- `src/dashboard.rs`
- `src/token_budget.rs` только если для truthful benchmark event surfacing это действительно необходимо

Что обязано появиться:
- unified machine-readable `statistics` block в benchmark/matrix payloads;
- поле `sample_size`;
- явная пара `baseline_run_id / candidate_run_id`;
- интервал неопределённости;
- drift summary;
- promotion verdict, который fail-closed уходит в `not_promotable`, если статистический блок неполон.

Exact statistical methods для `v1`:
- success/pass-rate metrics:
  - Wilson 95% confidence interval;
- score delta, mean delta, median latency, p95 latency:
  - bootstrap percentile 95% confidence interval;
  - bootstrap seed должен surface-иться в payload для воспроизводимости;
- discrete verdict/class distributions:
  - Jensen-Shannon divergence;
- continuous latency/score distributions:
  - Kolmogorov-Smirnov statistic;
- любой benchmark summary без `n` и explicit method metadata считается невалидным.

Что обязано materialize-иться в surfaces:
- `verify` JSON payloads;
- `observe snapshot`;
- dashboard benchmark/quality cards;
- MCP summaries для benchmark/matrix tools.

Что запрещено:
- объявлять improvement только по point estimate;
- прятать red drift за зелёный headline;
- писать CI/significance только в docs и не surface-ить их в runtime payload;
- выбирать другой статистический метод молча, если он не описан здесь.

Обязательный proof bundle:
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_observability.sh`
- companion non-regression по затронутым осям

Выход из Queue 1:
- benchmark contours публикуют `statistics` block;
- benchmark surfaces без статистического блока fail-closed не выдают promotable verdict;
- dashboard и observe не расходятся с raw payload truth.

#### Queue 2. Lifecycle transition discipline

Цель:
- сделать forgetting/lifecycle переходы first-class measured contour до запуска Markov advisory model.

Точные code and schema surfaces:
- `sql/000_bootstrap.sql`
- `src/forgetting.rs`
- `src/cli.rs`
- `src/main.rs`
- `src/observe.rs`
- `src/dashboard.rs`

Canonical state model для `v1` обязан быть именно таким:
- `active_hot`
- `active_stale`
- `pending_review`
- `compacted`
- `archived`
- `pruned`
- `protected`
- `quarantined`

Как выводить эти состояния:
- только из уже существующих полей и audit trails;
- без hidden synthetic truth-state;
- с explain trace, который показывает, почему запись попала именно в этот lifecycle-state.

Exact materialization contract:
- `ami.forgetting_audit_log` остаётся source-of-truth для forgetting actions;
- поверх него и `ami.memory_items` должен появиться derived transition dataset contract;
- canonical derived object для `v1`:
  - SQL view `ami.lifecycle_transition_events_v1`;
- canonical aggregated object для `v1`:
  - SQL materialized view `ami.lifecycle_transition_stats_v1`.

`ami.lifecycle_transition_events_v1` обязан surface-ить:
- `memory_item_id`
- `observed_state`
- `next_state`
- `dwell_ms`
- `derivation_kind`
- `retention_class`
- `decay_policy`
- `freshness_band`
- `utility_band`
- `access_band`
- `project_code`
- `namespace_code`
- `recorded_at`

Новые CLI surfaces:
- `cargo run -- memory transition-stats --project ... --namespace ...`
- `cargo run -- memory cohort-risk --project ... --namespace ...`

Dashboard/observe surfaces:
- governance-card `Жизненный цикл памяти` должна показывать не только aggregate count, но и cohort/state transition breakdown;
- `observe snapshot` должен иметь отдельный lifecycle-transition summary block.

Что запрещено:
- строить transition dataset в памяти без SQL-contract;
- смешивать все memory cohorts в одну усреднённую группу;
- тихо терять dwell-time или cohort features;
- оставлять lifecycle explainability только в коде без CLI/observe surface.

Обязательный proof bundle:
- `./scripts/proof_forgetting_consolidation.sh`
- `./scripts/proof_observability.sh`
- targeted Rust tests для lifecycle state derivation и SQL view/materialized view contract

Выход из Queue 2:
- transition dataset materialized и queryable;
- lifecycle surfaces объясняют переходы не хуже, чем текущий forgetting audit trail;
- existing Stage 9 safety invariants не нарушены.

#### Queue 3. Markov / hazard lifecycle v1

Цель:
- поверх transition discipline materialize-ить production advisory model, которая помогает planning/revalidation, но не получает truth-authority.

Точные code surfaces:
- `src/forgetting.rs` если изменений мало;
- если bounded-context вырастает, exact split filenames должны быть:
  - `src/lifecycle_transition.rs`
  - `src/lifecycle_markov.rs`
  - `src/lifecycle_explain.rs`
- wiring:
  - `src/cli.rs`
  - `src/main.rs`
  - `src/observe.rs`
  - `src/dashboard.rs`

Exact modeling contract для `v1`:
- модель cohort-separated;
- transition probabilities:
  - empirical transition counts;
  - Laplace smoothing `alpha = 1.0`;
- dwell-time block:
  - empirical `p50 / p75 / p90`;
- никаких hidden states, HMM, neural ranking или black-box replacement в `v1`.

Exact cohort split:
- `derivation_kind`
- `retention_class`
- `decay_policy`
- `freshness_band`
- `utility_band`
- `access_band`

Что обязано surface-иться:
- `expected_next_state`
- `pending_review_risk_7d`
- `archive_risk_30d`
- `prune_risk_30d`
- `expected_residency_ms`
- `cohort_reason_summary`

Новые CLI surfaces:
- `cargo run -- memory cohort-risk --project ... --namespace ...`
- `cargo run -- memory policy-simulate --project ... --namespace ...`

Dashboard/observe surfaces:
- governance-card `Жизненный цикл памяти` должна получить advisory block с expected next state и cohort risk;
- `observe snapshot` должен иметь `lifecycle_risk_summary`.

Что запрещено:
- давать модели право напрямую запускать prune/archive;
- выводить probabilistic lifecycle score как truth verdict;
- обходить existing policy/evidence protections;
- silently downgrade до другой модели без обновления этого документа.

Обязательный proof bundle:
- `./scripts/proof_forgetting_consolidation.sh`
- `./scripts/proof_observability.sh`
- targeted Rust tests для transition probability calculation, Laplace smoothing и cohort separation

Выход из Queue 3:
- Markov/hazard advisory contour production-visible;
- все outputs explainable;
- destructive authority по-прежнему у policy/evidence path, а не у модели.

#### Queue 4. Regression explain surface

Цель:
- получить объясняющий contour, который показывает, какие факторы связаны с helpful/stale/benchmark outcomes, не подменяя routing/truth.

Точные code surfaces:
- `src/retrieval_science.rs`
- `src/observe.rs`
- `src/dashboard.rs`
- `src/mcp.rs`

Если code volume вырастает:
- exact new module filename:
  - `src/regression_explain.rs`

Exact modeling contract для `v1`:
- binary outcomes:
  - logistic regression;
- continuous outcomes:
  - linear regression;
- surface только explanatory/reporting results;
- live routing, truth promotion или forgetting decisions на основе regression запрещены.

Minimum supported outcomes:
- `benchmark_pass`
- `stale_error`
- `retrieval_helpful`

Minimum surfaced metrics:
- `auc` для binary models;
- `brier_score` для binary probability outputs;
- `r_squared` для continuous outcomes, если они materialized;
- coefficient table;
- feature sign summary;
- sample size.

Новый CLI surface:
- `cargo run -- observe regression-explain --surface ...`

Dashboard/observe surfaces:
- dashboard explain card;
- `observe snapshot` explainability block.

Что запрещено:
- использовать regression score как authoritative ranking/truth source;
- публиковать explain model без quality metrics;
- учить модель на смешанном мусорном label contour без явного sample-size surface.

Обязательный proof bundle:
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_observability.sh`
- targeted Rust tests для feature extraction и report contract

Выход из Queue 4:
- explain surface production-visible;
- no hidden coupling with routing/truth/forgetting authority.

#### Queue 5. Poisson / arrival capacity forecast

Цель:
- materialize-ить forecast-only capacity contour для background jobs и arrival pressure, не превращая его в runtime enforcement law.

Точные code surfaces:
- `src/observe.rs`
- `src/dashboard.rs`
- `src/mcp.rs` если payload summary реально нужен наружу

Если code volume вырастает:
- exact new module filename:
  - `src/capacity_forecast.rs`

Exact modeling contract для `v1`:
- arrivals агрегируются минимум в `1m` и `5m` buckets;
- для каждого supported queue/job family surface-ится:
  - `lambda`;
  - `expected_arrivals`;
  - `poisson_interval_95`;
  - `observed_service_rate`;
  - `capacity_margin`;
- это forecast-only contour;
- никаких auto-throttle, auto-rejection или admission control на его основе в рамках этого документа.

Новый CLI surface:
- `cargo run -- observe capacity-forecast --window 5m`

Dashboard/observe surfaces:
- dashboard capacity/pressure card;
- `observe snapshot`;
- `observe sla-check` не должен расходиться с forecast contour по базовым метрикам.

Что запрещено:
- выдавать forecast за runtime truth;
- прятать sample window;
- публиковать lambda без source bucket definition.

Обязательный proof bundle:
- `./scripts/proof_load.sh`
- `./scripts/proof_observability.sh`
- `cargo run --release --quiet -- observe sla-check`
- targeted Rust tests для Poisson interval and bucket aggregation contract

Выход из Queue 5:
- capacity forecast surfaced в production как advisory/planning contour;
- runtime policy по-прежнему не зависит от него напрямую.

### Cross-queue hard rules

Для всех очередей без исключения:
- каждая semantic group коммитится отдельно;
- docs/governance не смешиваются с крупным runtime refactor без необходимости;
- если новый contour требует новый Rust module, prefer split over god-file growth;
- любая новая machine-readable surface должна иметь stable contract;
- dashboard нельзя обновлять без raw/JSON equivalent;
- observability payload не имеет права быть единственным доказательством без raw-result lane;
- implementer не имеет права считать `future-R&D-only` из таблиц ниже разрешением "остановиться"; authoritative stop/go contract задают именно `production scope` и `canonical execution order` этого раздела.

## Manual verdict on `AMAI_audit_and_improvement_plan.pdf`

### Что из PDF1 подтвердилось как live-useful

| Тезис | Ручной verdict | Почему |
| --- | --- | --- |
| `restore_confidence` остаётся в основном категориальным contour | `live-confirmed` | В текущем коде и surfaces всё ещё доминируют значения вроде `preliminary / medium / high / durable`, а не формальный calibrated belief-layer. |
| benchmark story не публикует статистическую честность уровня CI/significance/drift | `live-confirmed` | `benchmark coverage` по-прежнему показывает `0 materialized`, а gates/roadmap держат measured discipline, но не отдельный statistical benchmark layer с доверительными интервалами и hypothesis tests. |
| формального numeric posterior по memory/belief нет | `partially-confirmed` | В schema и отдельных surfaces есть `confidence`, но нет принятой canonical модели `truth/freshness/scope/usefulness posterior` с calibration policy. |
| крупные файлы и maintainability debt остаются фактором риска | `live-confirmed` | Giant-file debt остаётся частью текущего реального состояния проекта, хотя несколько bounded-context split уже materialized. |

### Что из PDF1 оказалось уже устаревшим snapshot-утверждением

| Тезис | Ручной verdict | Почему |
| --- | --- | --- |
| отсутствует `src/forgetting.rs` | `stale-snapshot` | Модуль уже materialized; эта претензия уже отдельно закрыта в manual audit verdict. |
| proof/onboarding contour ссылается на отсутствующие docs/scripts | `stale-snapshot` | Существенная часть gaps уже закрыта; repo содержит `agent_preflight`, maintainability guards, proof bundles и canonical docs. |
| в архиве не видно CI/workflow surfaces | `stale-snapshot` | В текущем repo есть `.github/workflows/repo-hygiene.yml` и machine-readable hygiene/proof/gate contour. |
| deterministic offline reproducibility не материализована | `stale-snapshot` | После последнего remediation repo уже содержит `.cargo/config.toml` и `vendor/`, а `proof_offline_no_run_build` пройден. |

### Что из PDF1 не надо дублировать как новый roadmap

| Тезис | Ручной verdict | Почему |
| --- | --- | --- |
| explainable forgetting и lifecycle важны | `duplicate-of-existing-roadmap` | Это уже зафиксировано в Stage 9 + `IMPLEMENTATION_GATES.md`. |
| evaluator/trust loop обязателен | `duplicate-of-existing-roadmap` | Это уже канонический Stage 10 baseline. |
| benchmark honesty и non-regression нельзя подменять красивыми цифрами | `duplicate-of-existing-roadmap` | Это уже locked law в roadmap, operations и maintainability contour. |

### Что из PDF1 полезно только как future-R&D direction

| Тезис | Ручной verdict | Почему |
| --- | --- | --- |
| Bayesian belief-layer для memory | `future-R&D-only` | Идея сильная, но сейчас нет materialized data/calibration/proof contour, чтобы честно объявлять её live roadmap item уровня implementation-ready. |
| Markov/hazard lifecycle для forgetting | `future-R&D-only` | Архитектурно совместимо с Amai, но требует measured transition data и audit-safe modeling. |
| Poisson arrival/capacity model | `future-R&D-only` | Полезно для future load/capacity planning, но пока не canonical runtime control layer. |
| regression as explain surface | `future-R&D-only` | Уместно как auxiliary explainability contour, но не как authoritative truth/ranking source. |

## Useful methodological primitives from `tv&ms_ff.pdf`

Из второго PDF не переносим формулы без контекста. Берём только методологические примитивы, которые можно положить на уже существующие contours `Amai`.

### 1. Independence-aware evidence

Что берём:
- независимость подтверждений нельзя угадывать по числу каналов;
- повтор одного и того же факта через зависимые surfaces не должен взрывать уверенность.

Куда ложится:
- provenance/evidence graph;
- retrieval reranking;
- conflict handling;
- будущий belief-layer.

Почему полезно для Amai:
- проект уже работает с provenance и truth-layers;
- это позволяет не подменять “много похожих следов” реальной независимой поддержкой.

### 2. Posterior-style confidence decomposition

Что берём:
- уверенность должна раскладываться не в одно декоративное число, а по разным смыслам.

Минимально уместные будущие компоненты:
- `truth_posterior`
- `freshness_posterior`
- `scope_posterior`
- `usefulness_posterior`

Что важно не нарушить:
- эти оценки не должны стать source of truth сами по себе;
- они могут помогать ranking/governance/restore, но не переписывать verified truth без policy/evidence path.

### 3. Estimation and confidence intervals

Что берём:
- benchmark/reporting не должен ограничиваться point estimates;
- при сравнении runs полезны uncertainty bands и sample-size-aware interpretation.

Куда ложится:
- memory benchmark reporting;
- compare-plane summaries;
- evaluator/gov surfaces.

### 4. Hypothesis testing and drift checks

Что берём:
- “кажется лучше” не годится;
- baseline vs candidate должен иметь explicit significance/drift interpretation.

Куда ложится:
- benchmark evaluation;
- non-regression discipline;
- telemetry drift reports.

### 5. Markov-style lifecycle thinking

Что берём:
- forgetting/consolidation полезно мыслить как систему explainable transitions, а не как набор ad-hoc cleanup heuristics.

Куда ложится:
- Stage 9 lifecycle/forgetting contour;
- retention/revalidation/review transitions;
- future policy simulation.

Как именно это должно materialize-иться в `Amai`, если contour дойдёт до реализации:
- это не truth-engine и не новый authoritative слой;
- это advisory/explainable lifecycle model поверх уже существующих `lifecycle_state`, `retention_class`, `decay_policy`, `consolidation_status`, `freshness_score`, `utility_score`, `access_count`, `last_accessed_at`;
- первый practical target не "общая умная математика", а более честный `revalidation / archive / prune` planner.

Exact v1 design contour:
- ввести небольшой canonical набор наблюдаемых lifecycle-состояний, а не строить одну огромную матрицу всех полей сразу;
- базовый кандидат для `v1`:
  - `active_hot`
  - `active_stale`
  - `pending_review`
  - `compacted`
  - `archived`
  - `pruned`
  - `protected`
  - `quarantined`
- эти состояния должны вычисляться из уже materialized state, а не жить отдельной неаудируемой сущностью;
- transition dataset должен собираться из:
  - `forgetting_audit_log`;
  - смен жизненных состояний `memory_items`;
  - access/freshness/revalidation traces;
- estimation contour должен стартовать с cohort-level transition statistics и hazard-style retention estimates, а не с black-box модели;
- модель обязана считать переходы по policy/class cohorts, а не усреднять всё в одну "среднюю память":
  - `derivation_kind`
  - `retention_class`
  - `decay_policy`
  - freshness/utility bands
  - access activity bands

Что эта модель должна давать:
- объяснимый прогноз, какая когорта памяти с высокой вероятностью уйдёт в `pending_review`, `archive` или `prune`;
- приоритизацию revalidation без blind threshold-spam;
- оценку expected residency time по состояниям;
- policy simulation для forgetting/review contour;
- capacity signal для background lifecycle jobs.

Жёсткие ограничения:
- Markov/hazard contour не имеет права объявлять, что запись "ложная" или "истинная";
- probabilistic output не может переписывать `verified truth`;
- модель не имеет права самостоятельно обходить policy/evidence path;
- `raw_capture / operator_write / verified_write_back / durable / legal_hold / retain_forever` должны оставаться защищёнными и вне destructive auto-promotion path;
- если observed data покажет, что pure Markov плохо описывает dwell-time, contour должен остаться `semi-Markov / hazard-first`, а не насильно упрощаться ради красоты.

### 6. Poisson-style arrival modeling

Что берём:
- load/arrival rate и background work queues полезно оценивать через arrival-model, а не только по вручную выбранным thresholds.

Куда ложится:
- capacity planning;
- queue sizing;
- background job pressure.

### 7. Regression as explain surface

Что берём:
- простая explain model может показывать, какие признаки связаны с useful recall, stale errors или evaluator outcomes.

Жёсткое ограничение:
- regression разрешена только как explain/forecast contour;
- regression не может стать authoritative truth/routing source без отдельного measured promotion path.

## Adoption matrix for Amai

### Adopt now as documentation/roadmap

| Что принимаем | Почему полезно | Почему не конфликтует с Amai | Куда ложится | Обязательный future proof path | Тип |
| --- | --- | --- | --- | --- | --- |
| Independence-aware evidence law | защищает от псевдо-подтверждений | усиливает provenance/truth, а не подменяет её | roadmap + synthesis-doc | evidence dependency traces, conflict/rerank validation | live roadmap item |
| Statistical benchmark honesty | делает compare/benchmark contour менее хрупким | продолжает current non-regression law | roadmap + gates | CI/intervals/significance/drift reports | live roadmap item |
| Explainable lifecycle transitions | усиливает Stage 9 forgetting correctness | совместимо с forgetting_audit_log и governance contour | roadmap + gates | transition audit, explain-forgetting, lifecycle traces | live roadmap item |
| Regression only as explain surface | даёт читаемый baseline без подмены truth | остаётся auxiliary contour | synthesis-doc + roadmap note | offline model report + non-authoritative surface checks | live roadmap item |

### Starting status before execution program

Этот backlog показывает starting status на дату ручной сверки.
Он не отменяет `canonical execution order` выше.

Правило для implementer:
- если contour входит в production scope и имеет очередь выше, его нужно реализовывать по execution program;
- если contour явно вынесен в out-of-scope этого документа, его нельзя молча "добрать заодно".

| Что ещё не materialized на момент ручной сверки | Почему тогда не было готово | Что должно появиться раньше |
| --- | --- | --- |
| Numeric posterior belief-layer | нет canonical calibration/data policy | measured datasets, calibration policy, proof contour |
| Calibrated restore confidence | current restore still categorical | restore telemetry, label-quality study, calibration harness |
| Markov/hazard lifecycle model | пока нет measured transition discipline | transition dataset, explainable policy model, audit-safe rollout |
| Poisson capacity model | пока это полезнее как forecasting contour, чем как runtime law | observed arrival metrics, queue telemetry, capacity validation |

### Already materialized / duplicate

| Пункт | Почему это duplicate |
| --- | --- |
| explainable forgetting и audit trail | уже лежит в Stage 9, gates и dashboard/governance surfaces |
| evaluator/trust loop | уже лежит в Stage 10 |
| benchmark honesty как product law | уже лежит в roadmap, operations и maintainability standard |

### Rejected / stale

| Пункт | Почему отклонён |
| --- | --- |
| missing `src/forgetting.rs` | snapshot-claim уже опровергнут текущим деревом |
| missing docs/scripts as current live defect | значительная часть gaps уже закрыта и заведена в hygiene contour |
| offline reproducibility absent | уже materialized через `.cargo/config.toml` + `vendor/` + proof |
| отсутствие workflow/gate contour | уже не соответствует текущему repo |

## Immediate roadmap-worthy items

Эти пункты стоит держать в canonical docs уже сейчас, но без объявления их implemented:
- `confidence/calibration` как future governance extension, а не текущий source of truth;
- `benchmark significance + drift` как next-layer discipline для compare/evaluator surfaces;
- `Markov/hazard lifecycle` как future extension для Stage 9, только через explainable transitions;
- `Poisson capacity` как future planning contour, не runtime truth;
- `regression explain surface` как optional explainability aid.

Целевые статусы для канонических docs:
- `confidence/calibration` -> `concept-only`
- `benchmark significance/drift` -> `planned`
- `Markov/hazard lifecycle` -> `concept-only`
- `Poisson capacity` -> `blocked by proof/data`
- `regression explain surface` -> `planned`

## Future research stack

### 1. Confidence and calibration

Потенциальный scope:
- restore confidence decomposition;
- memory item belief calibration;
- usefulness-vs-truth separation.

Нельзя выпускать без:
- calibration dataset;
- replayable evaluation;
- measured miscalibration reporting.

### 2. Statistical benchmark discipline

Потенциальный scope:
- interval-aware reports;
- significance-aware baseline comparisons;
- explicit drift reports per benchmark family.

Нельзя выпускать без:
- stable raw-run storage;
- baseline/candidate pairing;
- fail-closed interpretation rules.

### 3. Lifecycle modeling

Потенциальный scope:
- transition statistics;
- hazard/retention policies;
- revalidation scheduling.

Detailed design expectation:
- canonical state model должен быть малым, наблюдаемым и выводимым из уже существующих lifecycle/policy полей;
- первая practical unit анализа должна быть cohort, а не отдельная "магическая" memory-item score;
- runtime outputs должны идти в explain/forecast/recommendation surfaces, а не напрямую в destructive forgetting;
- до promotion нужно иметь measured answer на вопрос: улучшает ли contour `speed / accuracy / quality / truth`, а не только красиво ли выглядит transition matrix.

Нельзя выпускать без:
- explainable transition reasons;
- before/after audit trail;
- rollback-safe policy versioning.

Нельзя выпускать также без:
- явного transition dataset contract;
- observed validation на реальных forgetting/revalidation traces;
- surfaces для оператора:
  - expected next state;
  - why this cohort is at risk;
  - which policy feature drove the recommendation;
- fail-closed promotion rules, где advisory model не получает destructive authority автоматически.

### 4. Capacity and arrival modeling

Потенциальный scope:
- queue pressure forecasts;
- background job sizing;
- arrival-rate-aware capacity envelopes.

Нельзя выпускать без:
- observed queue/job telemetry;
- validation against real runtime traces;
- separation between forecast and enforcement.

## Rejected / stale / duplicate claims

### Rejected as stale snapshot

- `src/forgetting.rs` missing
- canonical docs/scripts missing as live repo truth
- no workflow/gate surfaces
- no offline reproducibility materialization

### Rejected as unsafe overreach

- любая попытка объявить Bayesian/Markov/Poisson слой готовым только по теоретической красоте;
- любая попытка заменить governed truth математической проекцией;
- любая попытка объявить regression/ranking-authoritative без measured promotion path.

### Marked as duplicate of existing canon

- explainable forgetting как correctness-law
- evaluator/trust loop как release-law
- benchmark honesty и non-regression как baseline discipline

## Итоговый вывод

Для `Amai` полезно брать из этих PDF не “готовую новую архитектуру”, а строго отфильтрованные усилители уже существующего каркаса:
- честную работу с зависимостью улик;
- более строгую benchmark discipline;
- explainable lifecycle modeling;
- осторожный research backlog по probabilistic layers.

Самое важное ограничение:
- пока у идеи нет contour mapping, measured proof path и совместимости с truth contract, она остаётся `concept-only` или `future-R&D-only`, а не превращается в новый обязательный live law проекта.
