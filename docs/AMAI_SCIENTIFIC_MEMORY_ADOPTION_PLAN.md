# Scientific Memory Adoption Plan For Amai

Дата ручной сверки: 2026-04-13

Источники:
- `AMAI_audit_and_improvement_plan.pdf`
- `tv&ms_ff.pdf`

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
- KAN-style reranking, routing или context-pack mutation без отдельного
  measured approval revision и no-authority proof bundle.

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

Fresh baseline law:
- if `proof_mcp_task_matrix.sh` or `proof_observability.sh` is red, this plan must not describe benchmark significance/drift or dashboard parity as fresh-green;
- current 2026-04-24 proof-refresh state is green after strict-heavy MCP matrix contamination preflight, explicit failed-run artifact handling and clean observability rerun.

Что надо зафиксировать как baseline artifact set:
- benchmark coverage summary;
- memory task matrix summary;
- MCP task matrix summary;
- observability snapshot summary;
- forgetting/governance dashboard snapshot или raw equivalent.

Canonical launcher `v1` для этого artifact set:
- `./scripts/scientific_queue0_baseline_freeze.sh`

Что он обязан materialize-ить:
- machine-readable manifest с `git_head`, initial/final worktree fingerprint, environment snapshot, dependency hashes, exact command invocations и per-command checksums;
- fail-closed классификацию `passed / pre_existing_known_failure / current_failure`;
- optional `--baseline-allowlist` contour для уже известных pre-existing red zones;
- optional `--require-clean-worktree` strict mode для freeze-run без TOCTOU-двусмысленности.

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

Текущий честный статус:
- `materialized_minimal_slice`
- bounded Queue 4 contour уже materialized в code/runtime:
  - `src/regression_explain.rs`
  - `cargo run -- observe regression-explain --surface ...`
  - `observe snapshot` block `regression_explain`
  - dashboard card `Regression explain`
- текущий proof bundle после fresh proof-refresh 2026-04-24:
  - `./scripts/proof_memory_task_matrix.sh` = green
  - `./scripts/proof_benchmark_contamination_preflight.sh` = green
  - `./scripts/proof_mcp_task_matrix.sh` = green after strict-heavy contamination preflight and explicit failed-run artifact handling
  - `./scripts/proof_observability.sh` = green after the MCP matrix dashboard compare lane was revalidated
    - it now also asserts the `Regression explain` dashboard card and `observe snapshot.regression_explain` guardrails
- fresh proof-refresh 2026-04-25:
  - `./scripts/proof_observability.sh` = green for the same read-only/fail-closed guardrail boundary, not for measured regression quality.
- current live sample остаётся honest fail-closed:
  - `sample_pool_size = 31`
  - `measured_outcomes = 0`
  - `insufficient_sample_outcomes = 3`
  - `status = unknown`
- текущее one-sided label distribution:
  - `benchmark_pass`: `n=17`, только positive class
  - `stale_error`: `n=31`, только negative class
  - `retrieval_helpful`: `n=31`, только positive class
- значит Queue 4 уже production-visible, но ещё не даёт measured regression quality на текущем live contour; это не code gap, а sample-shape/data gap.

#### Candidate Queue 4A. KAN-style context-pack utility explain surface

Статус:
- `research_candidate / spec_only / not_materialized`
- это не новая core queue и не shortcut вокруг Queue 0-5;
- все payload, rollout и proof поля ниже являются target contract, а не
  описанием уже существующего runtime behavior;
- future code must treat these fields as speculative/contract-based until a
  separate `shadow_approved` revision exists;
- any future consumer must be blocked by an explicit compile-time or runtime
  guard until `shadow_approved`; serializers, logs and helper projections must
  not leak these fields into live decision paths;
- этот раздел разрешает только contract/proof planning до отдельного
  measured approval revision;
- runtime/schema/dashboard/MCP implementation запрещена, пока этот раздел не
  будет явно promoted из `spec_only` в `shadow_approved`.

Цель:
- объяснить полезность уже разрешённых context-pack candidates;
- показать, почему конкретный chunk/source/symbol вошёл или не вошёл;
- найти redundant context, wasted tokens, weak-scope semantic hits,
  stale/conflicting evidence и нестабильные explanations;
- не менять сам context-pack, ordering, top-k, project/namespace scope,
  retrieval mode, truth state или lifecycle/forgetting decision.

Почему KAN/Kolmogorov-Arnold идея здесь уместна:
- полезная часть не в "магической" теореме, а в explainable additive/spline
  decomposition of feature interactions;
- planned surface would report operator-readable вклад факторов:
  `lexical_score`, `symbol_match`, `semantic_score`, `scope_guard`,
  `provenance_trust`, `freshness`, `token_cost`, `redundancy`,
  `evidence_conflict`, `continuity_relevance`;
- если KAN-style curves нестабильны или не выигрывают у simple baselines,
  contour остаётся research artifact и не получает product authority.

Allowed candidate scope:
- только candidates, которые уже прошли действующие project/namespace/scope
  фильтры;
- только traces после materialized context-pack decision;
- semantic/Qdrant evidence не может расширять scope или становиться единственным
  source of truth;
- lexical/symbol-first law сохраняется.

Exact authority contract:
- `truth_authority = false`
- `routing_authority = false`
- `ranking_authority = false`
- `runtime_authority = false`
- `forgetting_authority = false`
- `promotion_authority = false`

Минимальный trace contract:
- `context_pack_id`
- `correlation_id`
- `decision_trace_id`
- `project_code`
- `namespace_code`
- `retrieval_mode`
- `allowed_candidate_scope`
- `candidate_summary`
- `rerank_summary`
- `evidence_sufficiency`
- `final_decision`
- `model_visible_context_pack_payload_sha256`
- `candidate_order_sha256`
- `feature_schema_version`

Минимальный output payload:
- `context_pack_utility_explain.model_version`
- `surface_role = read_only_context_pack_utility_explain`
- `summary.status = measured | insufficient_sample | ood | not_materialized | unknown`
- `sample_pool_size`
- `outcomes`
- `top_contributors`
- `redundancy_summary`
- `token_utility_summary`
- `stability_summary`
- `ood_summary`
- `guardrails.truth_authority = false`
- `guardrails.routing_authority = false`
- `guardrails.ranking_authority = false`
- `guardrails.runtime_authority = false`
- `raw_parity.raw_payload_sha256`
- `raw_parity.dashboard_payload_sha256`
- `raw_parity.parity_status`

Rollout order:
1. `spec_only`: this section plus gate contract only; no code.
2. `trace_only`: capture/read existing retrieval/context-pack decision traces
   after pack construction; enabled-vs-disabled context-pack output must be
   byte/JSON equivalent.
3. `offline_replay`: train/evaluate candidate models only on stored traces,
   with simple transparent baselines.
4. `shadow_observe`: expose raw/observe summary as read-only, fail-closed
   projection.
5. `dashboard_internal`: add hidden/internal operator card only after raw parity,
   sample floor, OOD and SLA guards pass.
6. `user_visible_opt_in`: optional surface only after proof bundle and explicit
   approval; still advisory.

Failure semantics:
- missing raw trace -> `not_materialized`;
- one-sided labels -> `insufficient_sample`;
- stale schema/version mismatch -> `unknown`;
- OOD input -> `ood`;
- Qdrant unavailable -> no semantic-derived explanation, not best guess;
- raw/dashboard SHA mismatch -> dashboard card hidden and proof red;
- any scope mismatch -> fail-closed and no explanation.

Baseline comparisons:
- current heuristic/explain summary;
- simple linear/logistic or GAM-style baseline where applicable;
- KAN-style candidate;
- ablation by feature family;
- perturbation/stability check for top contributors.

Обязательный proof bundle before any promotion beyond `spec_only`:
- targeted Rust contract tests for trace schema, feature extraction and
  authority flags;
- enabled-vs-disabled context-pack equality check;
- adversarial empty/ambiguous trace tests;
- cross-project and namespace leak guard;
- Qdrant unavailable degradation test;
- redaction/secret hygiene check;
- baseline-vs-candidate raw-result lane with sample size and CI/drift metadata;
- `./scripts/proof_memory_task_matrix.sh`;
- `./scripts/proof_mcp_task_matrix.sh`;
- `./scripts/proof_observability.sh`;
- `cargo run -- observe snapshot`;
- `cargo run -- observe sla-check`.

Если implementation touches retrieval/vector/context-pack runtime, дополнительно:
- `./scripts/proof_accuracy.sh`
- `./scripts/proof_performance.sh`
- `./scripts/proof_load.sh`
- cold benchmark bundle from `docs/IMPLEMENTATION_GATES.md`
- external/Qdrant bundle from `docs/IMPLEMENTATION_GATES.md`

First approved implementation shape, only after `shadow_approved`:
- Rust-first module, exact candidate filename:
  - `src/context_pack_utility_explain.rs`
- observe/dashboard integration must read the raw advisory payload and must not
  compute hidden dashboard-only truth;
- any model artifact must be versioned and reversible;
- no new Python harness is allowed for internal proof.

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

Текущий честный статус:
- `materialized_minimal_slice`
- Queue 5 уже materialized как bounded forecast-only contour, а не как runtime control law.
- Canonical `v1` implementation now includes:
  - новый module/runtime path:
    - `src/capacity_forecast.rs`
  - новый CLI surface:
    - `cargo run -- observe capacity-forecast --window 5m`
  - new dashboard/read-only operator surface:
    - card `Capacity forecast`
  - new snapshot surface:
    - `observe snapshot.capacity_forecast`
  - new scoped history basis:
    - `system_snapshot` now persists project-local `_observability.scope_project_code / scope_namespace_code`
    - Queue 5 history source resolves as `project_scoped_observe_history`
- Current supported family set is intentionally narrow:
  - `nats_events`
- Live window state is intentionally data-dependent, not a stable completion claim:
  - `history_scope.mode = project_scoped_observe_history` is the durable status-truth claim;
  - measured/insufficient status can change as `system_snapshot` history ages;
  - latest raw spot-check on 2026-04-25 showed `history_points = 44`, `nats_events.1m = measured`, `nats_events.5m = insufficient_sample`, `sample_count = 3`;
  - these exact values are raw evidence for that run only, therefore docs must not claim `measured_families = 1`, `1m = measured`, `5m = measured` or any fixed capacity-quality number as a permanent live state.
- Current proof state:
  - targeted Rust tests for Poisson interval and bucket aggregation contract = green
  - targeted Rust tests for Queue 5 dashboard-card fallback and measured-window rendering = green
  - `./scripts/proof_observability.sh` = green in the fresh 2026-04-25 companion run.
    - it now also asserts the `Capacity forecast` dashboard card and `observe snapshot.capacity_forecast` guardrails
- Remaining honest gap:
  - Queue 5 is still a minimal advisory slice, not a broad multi-family capacity model and not a runtime enforcement contour.

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
| benchmark story не публикует статистическую честность уровня CI/significance/drift | `stale-snapshot` | Это утверждение было верным для более раннего snapshot, но уже не соответствует текущему repo: compare/drift layer теперь partially materialized, а `IMPLEMENTATION_STATUS.md` честно фиксирует measured CI/significance/drift slices для `memory_task_matrix` и `mcp_task_matrix`, хотя contour ещё не объявлен полностью завершённым. |
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

### Что из PDF1 осталось future-only или стало частично materialized

Эта таблица больше не является "всё future-only" snapshot. После Queue 1-5
часть идей уже имеет bounded production-visible slices. Статус ниже фиксирует
текущую границу: что реально surfaced/proved, а что всё ещё нельзя объявлять
готовым runtime/truth/policy layer.

| Тезис | Ручной verdict | Почему |
| --- | --- | --- |
| Bayesian belief-layer для memory | `future-R&D-only` | Идея сильная, но сейчас нет materialized data/calibration/proof contour, чтобы честно объявлять её live roadmap item уровня implementation-ready. |
| KAN-style context-pack utility explain surface | `research_candidate / spec_only / not_materialized` | Полезная часть ограничена read-only explanation of already allowed context-pack candidates. Это не runtime ranking/truth layer и не roadmap promise до `shadow_approved` revision plus proof bundle. |
| Markov/hazard lifecycle для forgetting | `materialized_minimal_slice / advisory-only` | Stale как pure future claim: Queue 3 уже surfaced через `memory cohort-risk`, governance/dashboard summary и MCP/`observe snapshot` summary. Не закрыто как broader policy-simulation/approval layer и не получает destructive authority. |
| Poisson arrival/capacity model | `materialized_minimal_slice / forecast-only` | Stale как pure future claim: Queue 5 уже surfaced для `nats_events` через `src/capacity_forecast.rs`, `observe capacity-forecast`, dashboard card и snapshot block. Broader multi-family capacity model и runtime enforcement всё ещё blocked by proof/data. |
| regression as explain surface | `materialized_minimal_slice / insufficient_sample_for_measured_quality` | Stale как pure future claim: Queue 4 уже surfaced через `src/regression_explain.rs`, `observe regression-explain`, snapshot block и dashboard card. Live measured model quality не достигнута, потому что текущий sample одноклассный/`insufficient_sample`. |

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

### 8. KAN-style additive feature interaction as context-pack utility explanation

Что берём:
- explainable additive/spline feature interaction surface для already allowed
  context-pack candidates;
- decomposition of token utility, redundancy, scope risk, provenance support,
  semantic/lexical/symbol interaction and evidence conflict;
- stability/OOD reporting for the explanation itself.

Куда ложится:
- Candidate Queue 4A;
- retrieval/context-pack diagnostics;
- operator explainability near `Regression explain`, not a benchmark card and
  not a truth layer.

Жёсткое ограничение:
- KAN-style output is advisory projection only;
- it cannot change retrieval ordering, candidates, top-k, scope, truth,
  retention, task state or dashboard truth;
- if it does not beat simple transparent baselines under held-out proof and
  stability checks, it remains research-only.

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

| Что было не materialized на момент ручной сверки | Почему тогда не было готово | Текущий status-truth | Что должно появиться дальше |
| --- | --- | --- | --- |
| Numeric posterior belief-layer | нет canonical calibration/data policy | `future-R&D-only` | measured datasets, calibration policy, proof contour |
| Calibrated restore confidence | current restore still categorical | `concept-only / blocked-by-calibration-data` | restore telemetry, label-quality study, calibration harness |
| KAN-style context-pack utility explain surface | no approved trace schema, model stability proof or no-authority gate yet | `research_candidate / spec_only / not_materialized` | Queue 4A shadow contract, enabled-vs-disabled equality proof, OOD/stability guards, dashboard/raw parity |
| Markov/hazard lifecycle model | не было measured transition discipline | `materialized_minimal_slice / advisory-only` через Queue 3; bounded `policy-simulate` CLI now materialized, but broader measured approval contour still pending | validation that recommendations improve revalidation/forgetting discipline, approval path before any policy authority |
| Poisson capacity model | полезнее как forecasting contour, чем runtime law | `materialized_minimal_slice / forecast-only` через Queue 5 for `nats_events`; broader multi-family model/runtime enforcement pending | observed multi-family arrival metrics, queue telemetry validation, explicit policy approval before enforcement |

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

Эти пункты стоит держать в canonical docs уже сейчас, но без ложного возврата в pre-materialized состояние:
- `confidence/calibration` как future governance extension, а не текущий source of truth;
- `KAN-style context-pack utility explain` как `research_candidate / spec_only`
  extension candidate under Queue 4A, not a core roadmap promise;
- `benchmark significance + drift hardening` как следующий disciplined hardening layer поверх уже materialized compare/evaluator surfaces, а не как ещё не начатую идею;
- `Markov/hazard lifecycle` как уже materialized minimal advisory slice для Stage 9, включая bounded `memory policy-simulate` review contour поверх explainable transitions; broader measured approval/policy workflow still pending;
- `Poisson/arrival capacity forecast` как уже materialized minimal advisory slice, а более широкий `Poisson capacity model` как future planning contour, не runtime truth;
- `regression explain surface` как уже materialized minimal explainability aid, а не как purely future option.

Целевые статусы для канонических docs:
- `confidence/calibration` -> `concept-only`
- `KAN-style context-pack utility explain` -> `research_candidate / spec_only / not_materialized`
- `benchmark significance/drift` -> `in_progress / fresh-proof-green / measured-approval-human-gated`
- `Markov/hazard lifecycle` -> `materialized_minimal_slice / advisory-only`
- `Poisson/arrival capacity forecast` -> `materialized_minimal_slice`
- `Poisson capacity model` -> `blocked by proof/data`
- `regression explain surface` -> `materialized_minimal_slice`

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
