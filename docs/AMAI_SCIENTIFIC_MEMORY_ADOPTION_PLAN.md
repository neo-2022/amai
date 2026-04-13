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

### Future R&D backlog

| Что откладываем | Почему не сейчас | Что должно появиться раньше |
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

Нельзя выпускать без:
- explainable transition reasons;
- before/after audit trail;
- rollback-safe policy versioning.

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
