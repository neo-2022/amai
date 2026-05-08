# Статус реализации Amai

## Зачем нужен этот документ

Этот документ нужен не для общей архитектуры и не для длинных рассуждений.

Он нужен на один простой вопрос:
- что уже сделано;
- что сейчас в работе;
- что ещё не сделано;
- какой этап следующий.

Агент не должен вычислять это сам по всему корпусу документов.

## Как им пользоваться

Если агент впервые заходит в проект, порядок такой:

1. Прочитать `AGENTS.md`.
2. Обновить machine-readable preflight snapshot:
   - `./scripts/agent_preflight.sh --json`
3. Прочитать `docs/AGENT_START_HERE.md`.
4. Прочитать этот файл.
5. Только потом идти в `ARCHITECTURE`, `OPERATIONS`, `AMAI_GLOBAL_MEMORY_ROADMAP` и частные планы.

Простое правило:
- этот файл отвечает на вопрос `где проект сейчас`;
- roadmap отвечает на вопрос `куда проект идёт`;
- architecture/operations отвечают на вопрос `как устроен текущий baseline и какие у него законы`.
- для значимого stage-close machine-readable след maintainability gate лежит в `.amai/onboarding/project-maintainability-gate-state.json`.
- для значимого обновления этого файла обязателен passing `./scripts/implementation_status_sync_guard.sh --json`.
- checkbox любого этапа запрещено ставить, пока не прогнан весь уже materialized и подходящий benchmark/proof bundle этого этапа и его соседних shared contours.
- если benchmark contour публикуется на dashboard, checkbox любого этапа запрещено ставить, пока не перепроверена и сама dashboard surface этого результата.

## Как агент должен работать с этим файлом

Любой агент должен использовать этот файл как главный быстрый status snapshot.

То есть:
- не вычислять статус проекта по косвенным признакам;
- не пытаться понять прогресс только по коду или по случайным кускам roadmap;
- сначала открыть этот файл;
 - если изменение stage-based или затрагивает critical zone, сначала ещё пройти `./scripts/maintainability_gate.sh --json`;
- при желании не вручную, а через `.amai/onboarding/project-agent-preflight-state.json`;
- потом открыть нужный этап по checkbox-ссылке;
- затем открыть matching section в `IMPLEMENTATION_GATES.md`;
- не ждать, пока пользователь отдельно напомнит про benchmark из подходящего bundle;
- перед stage-close сверить, что прогнан весь подходящий benchmark/proof bundle, а не только один удобный blocking-proof;
- перед stage-close отдельно проверить dashboard-card/snapshot для всех benchmark contours, которые туда публикуются;
- и только потом идти в профильный документ и в код.

В терминах decision tree:
- это не корень;
- это ствол;
- отсюда агент уходит в нужную ветвь работы и сюда же возвращается после каждого значимого подшага.

## Каноническая лестница реализации

Именно этот файл считается `SSOT` для порядка работ.

Здесь агент обязан проверять не только `что уже сделано`, но и `с какого уровня вообще
разрешено начинать текущую работу`.

Для `Amai` лестница такая:

1. фундамент:
   - truth/source-of-truth;
   - memory fabric;
   - continuity;
   - working state;
   - task / commitment truth;
   - restore semantics;
   - provenance и scope boundaries;
2. механика продолжения работы:
   - startup;
   - handoff;
   - reconnect;
   - delivery / client surface transitions;
   - новая чистая рабочая поверхность;
3. доказательные и объясняющие надстройки:
   - benchmark/eval;
   - scientific queues;
   - explainability;
   - forecast;
   - probabilistic/statistical advisory layers;
4. поверхности показа:
   - dashboard;
   - compare surface;
   - onboarding wording;
   - operator hints;
   - human-readable summaries.

Жёсткий закон:
- верхний слой нельзя полировать как отдельную workline, пока нижний слой не выровнен;
- scientific contour не заменяет основную Stage-лестницу и не даёт shortcut вокруг foundation;
- surface/dashboard работа допустима только как:
  - синхронизация после изменения нижнего слоя;
  - или исправление явной truth-путаницы в уже materialized surface.

Что считается нарушением лестницы:
- dashboard-first polish без изменения foundation/delivery/proof слоя;
- user-facing claim раньше machine-readable контракта и proof lane;
- попытка сделать scientific overlay “вторым roadmap”;
- попытка трактовать advisory/read-only surface как самостоятельный truth-layer.

## Текущий общий статус

### Общая оценка

Проект находится в состоянии:
- сильный current-state baseline уже materialized;
- target-state memory fabric уже хорошо спроектирован;
- кодовая реализация target-state уже начата по этапам;
- `Этапы 0-10` закрыты по текущему internal status checklist и fresh proof bundle;
- prior MCP matrix / observability red-state закрыт: strict-heavy preflight, explicit failed-run artifact и clean rerun proof bundle materialized;
- текущий implementation focus не отменяет foundation-first лестницу:
  scientific reinforcement overlay, status-truth и surface-sync работа допустимы только поверх уже
  выровненных foundation/delivery contours, а не как отдельная dashboard-first линия;
- свежий эксплуатационный verdict по любому `closed / green / materialized` claim берётся только из matching proof bundle, dashboard/raw lane и machine-readable artifact, а не из старой формулировки в этом файле.

### Что уже точно сделано

Уже materialized и считается baseline:
- `continuity startup` как обязательный machine-readable front door;
- `agent preflight` как machine-readable doc/status front door;
- `working-state` baseline;
- `chat-start restore` baseline;
- первый durable `ExecCtl` contour:
  - `project_task_ledger`;
  - `pending_return`;
  - `active lease`;
- `PostgreSQL` как truth-source;
- lexical/symbol retrieval + semantic accelerator;
- install/bootstrap/onboarding контуры;
- client onboarding для `Hermes` теперь materialize-ит не только MCP config и `.hermes.md`, но и sticky project-bound profile, чтобы repo-scoped `Amai` startup подхватывался по умолчанию даже вне repo `cwd`;
- `Hermes` startup surface ужат до compact contract-pointer: `.hermes.md` и managed profile `SOUL.md` больше не дублируют длинный procedural startup-law и меньше раздувают prompt на слабых hostах;
- benchmark registry и measured matrix contours;
- live/proof/verify token separation;
- fail-closed startup contract;
- общий архитектурный target-state уже разложен в master-roadmap;
- compare-plan и task-plan уже встроены как частные модули общего roadmap.

## Что сейчас в работе

### Этап 1. Scope и identity control plane

Текущий честный статус:
- этап закрыт;
- stage-local control plane и companion retrieval / isolation / hostile contours прогнаны полностью;
- checkbox можно держать закрытым, пока следующий значимый change не откроет новый gap.

Что уже materialized в рамках этапа:
- `workspace` truth-layer;
- `team` truth-layer;
- `transfer_policy` truth-layer;
- `import_packet` truth-layer;
- расширение `project register` через `workspace` и `visibility_scope`;
- расширение `relation add` через `project_link_type`, `relation_status`, `requires_approval`, `transfer_policy`;
- отдельный proof `./scripts/proof_scope_identity_control_plane.sh`.
- fail-closed surface guard `./scripts/scope_identity_surface_guard.sh --json`.

Что уже прошло:
- targeted Rust CLI tests на новые defaults и relation fields;
- `./scripts/proof_scope_identity_control_plane.sh`;
- `./scripts/scope_identity_surface_guard.sh --json`;
- `./scripts/proof_project_registration_canonicalization.sh`;
- `./scripts/proof_project_relocation_contour.sh`;
- `./scripts/proof_hostile.sh`;
- `./scripts/proof_memory_task_matrix.sh`;
- `./scripts/proof_accuracy.sh`.
- `./scripts/proof_performance.sh` в полном режиме `warmup=3`, `iterations=20`;
- `./scripts/proof_cold_benchmark.sh` как proof full-cold contour;
- `./scripts/proof_cold_benchmark_self_contained.sh` как repo-local self-contained fixture tier без внешнего corpus;
- `./scripts/proof_cold_benchmark_canonical.sh` как canonical large repo-pool cold contour, который имеет право обновлять `latest_cold_path_benchmark`;
- `./scripts/proof_load.sh` как dashboard-visible hot-load contour.
- retrieval-side оптимизация scoped exact-document lookup в cold path с повторным полным benchmark bundle после правки.
- `./scripts/proof_external_benchmark_env.sh`;
- raw-result lane из `./scripts/proof_external_benchmark_adapter.sh`: upstream `ann-benchmarks` сейчас держит canonical qdrant launch path как `disabled=true`, поэтому contour зафиксирован как external harvest / adapter-readiness, а не как fake green dashboard-card.
- dashboard-check через `/api/dashboard` для четырёх benchmark-card:
  - `Hot Load Benchmark / latest_retrieval_load_hot` = `pass`;
  - `Hot Retrieval Benchmark / latest_retrieval_hot` = `pass`;
  - `Cold End-to-End Benchmark / latest_cold_path_benchmark` = `pass`;
  - `Accuracy / Isolation Verification / latest_retrieval_accuracy` = `pass`.
- external/Qdrant contour сейчас подтверждён raw-result lane, а не dashboard-card:
  - `benchmark_qdrant` в текущем `/api/dashboard` = `null`, поэтому dashboard-check для него сейчас неприменим;
  - Stage 1 использует разрешённый raw external harvest verdict вместо несуществующей карточки;
  - readiness/env contour = `ok`, adapter contour = `upstream_disabled_default_path`, что честно задокументировано и не маскируется под продуктовый regression.

Что подтвердило закрытие этапа:
- performance contour больше не блокирует stage-close:
  - canonical cold contour теперь `TARGET MET`;
  - свежий canonical raw-result: `P50 = 0.965 ms`, `P95 = 1.351 ms`, `P99 = 1.736 ms`, `Max = 2.149 ms`, `sample_count = 1105`;
- retrieval dashboard surface после полного rerun зелёная:
  - `Hot Load` = `pass`;
  - `Hot Retrieval` = `pass`;
  - `Cold End-to-End` = `pass`;
  - `Accuracy / Isolation` = `pass`;
- maintainability / closure guards на текущем worktree зелёные:
  - `./scripts/maintainability_gate.sh --json` = `PASS`;
  - `./scripts/maintainability_stage_close_guard.sh --json` = `checkbox_closure_allowed: true`.

Значит:
- stage-local proofs для scope/identity, hostile, memory/isolation и accuracy не дают права закрыть этап без полного performance contour;
- hot-load contour тоже нельзя пропускать, потому что scope/visibility влияют на retrieval-plane throughput и dashboard benchmark surface;
- cold-path тоже нельзя проверять только одной поверхностью: micro cold contour, proof full-cold contour и canonical `latest_cold_path_benchmark` обязательны вместе;
- self-contained cold contour теперь нельзя пропускать, если задача заявляет reproducibility/clean-machine story для cold benchmark: он не заменяет canonical large repo-pool contour, а страхует repo-local mandatory path без внешних repo-зависимостей;
- vector/Qdrant lane тоже нельзя пропускать, если touched surface задел retrieval/vector contour: тогда обязательны `proof_external_benchmark_env.sh`, `proof_external_benchmark_adapter.sh` и raw harvest/result verdict;
- любой stage-close benchmark теперь обязан идти в полном режиме, а не в smoke-режиме;
- свежий полный benchmark bundle прогнан и retrieval dashboard surface сейчас зелёная;
- полная функциональная готовность `Этапа 1` по roadmap и companion bundle подтверждена, поэтому этап считается `closed`.

### Что уже закрыто на уровне дизайна

Архитектурно уже закрыто и не должно обсуждаться заново:
- `graph-first` для task-memory;
- модульная типизированная память вместо одного generic store;
- `scope / identity` модель;
- provenance/evidence ladder;
- temporal truth;
- `workspace_restore_pack`;
- procedural memory как executable skill memory;
- safety/privacy/poisoning baseline;
- forgetting/consolidation/pruning;
- compare/benchmark plane;
- stage-gate и migration/kill-switch laws.

## Чеклист этапов

Ниже короткий чеклист всей последовательности работ.

Его смысл:
- агент не гадает, что уже закрыто;
- после закрытия этапа здесь просто меняется checkbox;
- это самый быстрый статус-срез по проекту.

- [x] [Этап 0. Зафиксировать новую общую модель memory fabric](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-0-зафиксировать-новую-общую-модель)
- [x] [Этап 1. Scope и identity control plane](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-1-scope-и-identity-control-plane)
- [x] [Этап 2. Typed memory envelope + provenance](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-2-typed-memory-envelope--provenance)
- [x] [Этап 3. Commitment / task graph](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-3-перевести-task-tree-в-commitment-graph)
- [x] [Этап 3A. Ранний procedural seed contour](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-3a-ранний-procedural-seed-contour)
- [x] [Этап 4. Workspace restore pack](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-4-собрать-workspace-restore-pack)
- [x] [Этап 5. Semantic + temporal memory strengthening](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-5-semantic--temporal-memory-strengthening)
- [x] [Этап 6. Multi-agent shared/private memory](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-6-multi-agent-sharedprivate-memory)
- [x] [Этап 7. Compare + benchmark plane](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-7-compare-and-benchmark-plane)
- [x] [Этап 8. Procedural memory](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-8-procedural-memory)
- [x] [Этап 9. Forgetting, consolidation, pruning](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-9-forgetting-consolidation-pruning)
- [x] [Этап 10. Governance, safety, evaluator loop](AMAI_GLOBAL_MEMORY_ROADMAP.md#этап-10-governance-safety-evaluator-loop)

### Status-truth audit overlay

Этот слой добавлен после cross-doc аудита документации.

Он нужен, чтобы `закрыто` не превращалось в поверхностную отметку:
- checkbox выше означает `internal stage closure зафиксирован в status snapshot`;
- свежий claim `работает / green / доказано` разрешён только после rerun matching proof bundle из `IMPLEMENTATION_GATES.md`;
- если fresh proof не прогнан в текущей рабочей линии, claim должен маркироваться как `proof-refresh-required`;
- если proof красный, checkbox и связанный `materialized` claim снимаются до root-cause, fix и повторного proof;
- если contour surfaced в dashboard, обязательно сверять dashboard-card с raw/source lane;
- если contour не surfaced в dashboard, использовать raw-result lane и прямо писать, что dashboard-check неприменим;
- external memory benchmark registry из `IMPLEMENTATION_GATES.md` блокирует claim `external benchmark-grade long-term memory maturity`, пока соответствующие source/prep/real-runtime/real-score/upstream-parity lanes не materialized.

Текущие audit findings, которые нельзя замалчивать:
- `AGENT_START_HERE.md` был устаревшим entrypoint и продолжал вести агента к `Этапу 8` / `Этапу 1`; это считается documentation source-of-truth defect, а не runtime feature gap.
- Старые `#L...` ссылки из checklist в roadmap дрейфовали и вели на неправильные разделы; checklist переведён на heading anchors.
- Stage 0-9 остаются internal-closed, но Stage 8-9 и scientific queues требуют fresh proof refresh перед любым новым публичным claim `green сейчас`.
- Stage 10 restored to fresh-green after the MCP matrix red-state/dashboard no-data defect was fixed and cleanly rerun.
- Queue 1 остаётся `in_progress`: measured approval human-gated, automatic promotion запрещён.
- `KAN-style context-pack utility explain` уже зафиксирован в
  `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` как `Candidate Queue 4A`, но только
  со статусом `research_candidate / spec_only / not_materialized`; это docs/spec
  status, а не runtime materialization и не dashboard-visible contour.
- Queue 4 regression explain production-visible, но live measured regression quality ещё не достигнута из-за `insufficient_sample`.
- Queue 5 capacity forecast production-visible как forecast-only/read-only surface, но конкретные live window values (`history_points`, `sample_count`, measured/insufficient state) являются raw-result observation, а не долговечным status claim.
- Бесшовный host-side переход длинной multi-line рабочей линии в новую чистую рабочую поверхность по всем client/host средам остаётся главным product gap.
- Compatibility `memory search` bridge explainability теперь proof-backed: zero-hit/cache-hit path тоже печатает `Почему вошло` и `Почему часть не вошла`, а proof больше не использует stale release binary.

Fresh proof-refresh 2026-04-24:
- `./scripts/proof_procedural_benchmark.sh` — green.
- `./scripts/proof_forgetting_consolidation.sh` — green.
- `./scripts/proof_hostile.sh` — green.
- `./scripts/proof_memory_task_matrix.sh` — green.
- `./scripts/proof_benchmark_contamination_preflight.sh` — green.
- `./scripts/proof_mcp_task_matrix.sh` — green after strict-heavy contamination preflight and explicit failed-run artifact handling were added.
- `./scripts/proof_observability.sh` — green after dashboard `MCP task matrix compare` was revalidated from the fresh MCP matrix snapshots.
- `./scripts/proof_memory_external_benchmarks.sh` — green for external memory dataset download, adapter workspace preparation, normalized case preparation and a synthetic runtime+baseline-score smoke only; this does not prove full scored external benchmark maturity.

Fresh proof-refresh 2026-04-25:
- `./scripts/proof_memory_external_benchmarks.sh` — green again for the same bounded evidence lane: source/tool preflight, dataset download, adapter workspace creation, normalized cases for LongMemEval/MemoryAgentBench/LoCoMo and one synthetic `external-memory-run` + `external-memory-score` exact-match smoke.
- The proof output still states LongMemEval/MemoryAgentBench/LoCoMo need full real dataset runtime+score proof and upstream scorer parity before any `external benchmark-grade long-term memory maturity` claim.
- Operational boundary observed during the rerun: MemoryAgentBench `accurate_retrieval` generated large prep artifacts (about 6.8 GiB across `cases.jsonl` and `requests.jsonl` for 2000 cases). This is acceptable prep evidence, but not evidence that a lightweight regular full benchmark/runtime lane is materialized.
- `./scripts/proof_observability.sh` — green for observability/dashboard guardrails, including Queue 4 `Regression explain` and Queue 5 `Capacity forecast` cards plus raw snapshot contracts. This refresh proves read-only/fail-closed surface presence, not measured regression quality or durable measured capacity quality.

Fresh bounded real runtime proof 2026-04-26:
- `./scripts/proof_memory_external_real_bounded.sh` — green for bounded, dataset-specific LongMemEval `longmemeval_s_cleaned` runtime+baseline-score evidence: limited real normalized requests, real `external-memory-run` predictions, completed runtime status, case metrics, retrieval-backed relaxed query retry and `external-memory-score` output.
- The bounded runtime metrics now include `answer_source_boundary` (`external_memory_answer_source_boundary_v1`) so retrieval-backed answers and fallback-scan-assisted answers are accounted separately; this is answer-source accounting, not semantic precision maturity.
- The bounded runtime metrics now also include `retrieval_relevance_boundary` (`external_memory_retrieval_relevance_boundary_v1`) so retrieved snippets are checked by a query-overlap proxy; this is retrieval relevance accounting, not gold-labeled semantic precision maturity.
- The bounded runtime metrics now also include `gold_answer_relevance_boundary` (`external_memory_gold_answer_relevance_boundary_v1`) so retrieved snippets are checked for lexical support of the benchmark `answer` label; this proves bounded gold-label data flow only, not semantic precision maturity. The current bounded proof keeps zero support cases blocker-visible instead of pretending answer-bearing retrieval has been proven.
- The bounded score output now also includes `official_scorer_boundary` (`external_memory_official_scorer_boundary_v1`) for the LongMemEval official scorer contract: upstream script paths, expected input fields, task types and `gpt-4o-2024-08-06` metric model are machine-visible, while live judge execution and upstream metrics reconciliation remain blocker-visible.
- The official scorer lane now has Rust-only `external-memory-official-judge` execution/log materialization (`external_memory_official_judge_execution_v1`) plus `./scripts/proof_memory_external_official_judge.sh`; it embeds the upstream LongMemEval answer-check prompt templates and keeps no-live, missing-key and non-official-model cases fail-closed without writing a synthetic live eval log.
- The official judge lane now also has local API-failure proof, `./scripts/proof_memory_external_official_judge_api_failure.sh`: fake HTTP 429/503 Chat Completions-compatible responses, HTTP 200 malformed/empty-choices/missing-content response-contract failures and a connection-refused transport path must produce blocked summaries, leave eval-results absent, classify the failure, and keep the dummy key value out of the summary even if a hostile fake error body echoes it. This is deterministic failure-contract evidence, not live provider evidence.
- The official scorer lane now also has a bounded live-operator wrapper, `./scripts/proof_memory_external_official_judge_live_bounded.sh`, for the real `longmemeval_s_cleaned` artifacts. The current local run is blocked by missing `OPENAI_API_KEY`, proves `official_judge_api_key_not_materialized`, and deliberately leaves the official live eval-results log absent.
- The bounded live-operator lane now has a six-type variant, `./scripts/proof_memory_external_official_judge_live_balanced.sh`: it selects one raw LongMemEval record for each official question type, normalizes via `external-memory-prepare --source-path`, runs six Amai predictions, and then proves the same no-key official judge fail-closed boundary. This fixes the first-N sample gap where the first 3 cases covered only `single-session-user`.
- The key-backed official judge guard now has a Rust-native secret non-persistence verifier: every regular file in the proof output directory is checked for the configured API key value, and any match is a hard proof failure. Local no-key proof still cannot exercise that branch without an authorized key.
- The no-key/offline official judge guard now also asserts that `[REDACTED_OFFICIAL_JUDGE_API_KEY]` is absent from default proof, missing-key bounded and balanced summaries; the marker is valid only when a materialized key value was actually removed from hostile fake-provider or key-backed response output.
- This artifact-hygiene guard is explicitly not a live/API success claim and does not advance official upstream scorer parity without an authorized live judge run.
- Rust regression coverage now also checks that no-live, missing-key and model-mismatch official judge summaries stay marker-free.
- The redaction marker definition is explicit in both Rust and shell proof surfaces, so future marker changes are audited instead of silently drifting across tests.
- The official scorer lane now also has Rust-only `external-memory-official-score` reconciliation (`external_memory_official_score_reconciliation_v1`) plus `./scripts/proof_memory_external_official_score_reconcile.sh`; it validates upstream-style eval-results JSONL and computes official-style metrics, but still does not prove live judge provenance or upstream parity.
- This is not full external benchmark-grade maturity: the proof is limited by `AMAI_EXTERNAL_MEMORY_REAL_LIMIT`, uses the internal exact/contains/abstention baseline scorer, and keeps official upstream scorer parity open.

Closed incident from the same audit:
- prior red run: `MCP task matrix p95_ms=26764.086 exceeds allowed 25000.000` while an unrelated heavy `ollama runner` contaminated the benchmark window;
- prior dependent red: dashboard card `MCP task matrix compare` rendered `ещё нет данных` after the failed MCP matrix proof;
- fixed behavior: latency-sensitive MCP matrix proof now runs a strict heavy-process contamination preflight before deleting matrix snapshots, and `mcp_task_matrix` records gate failures into the observability payload before returning non-zero.

External memory benchmark status-truth from the same audit:
- fixed proof-harness defect: LoCoMo no longer passes as `10` empty cases with missing question/context/answer; converter now expands `qa[]` and renders `conversation.session_*` as context;
- materialized prep lanes: LongMemEval, MemoryAgentBench and LoCoMo have non-empty normalized cases and requests guarded by `proof_memory_external_benchmarks.sh`;
- materialized command-contract smoke: `external-memory-run` and `external-memory-score` are exercised on one synthetic exact-match case;
- materialized bounded real runtime+score lanes: LongMemEval `longmemeval_s_cleaned`, AMA-Bench `ama_bench_manual`, and MemoryAgentBench `memoryagentbench_conflict_resolution` plus `memoryagentbench_long_range_understanding` and `memoryagentbench_test_time_learning` now have initial, bounded, dataset-specific real runtime prediction, answer-source accounting and baseline-score proof through `proof_memory_external_real_bounded.sh`, `proof_memory_external_real_bounded_ama_bench.sh`, and `proof_memory_external_real_bounded_memoryagentbench.sh`; fresh reruns also materialize and bounded-proof-check the joint invariant `top_ranked_relevance_and_gold_answer_supported_retrieval_cases <= top_ranked_gold_answer_supported_retrieval_cases` for those named MemoryAgentBench profiles, plus explicit boolean-typed `runtime_corpus_sha256` / `runtime_corpus_reused_from_previous_case` accounting. On the current default bounded slice (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`), the truthful split is profile-specific rather than global: `memoryagentbench_conflict_resolution` and `memoryagentbench_test_time_learning` prove same-run identical-corpus reuse (`runtime_corpus_unique_sha_count=1`, `runtime_corpus_reused_cases=2`, reused cases keep `index_project_ms=0`), while `memoryagentbench_long_range_understanding` proves the opposite bounded fact (`runtime_corpus_unique_sha_count=3`, `runtime_corpus_reused_cases=0`) because each bounded case materializes a different runtime corpus. This remains bounded operational evidence only and is not a general cache/reproducibility maturity gate;
- materialized bounded blocked profile: MemoryAgentBench `memoryagentbench_accurate_retrieval` now has a bounded end-to-end runtime+baseline-score blocked proof through `proof_memory_external_real_bounded_memoryagentbench_accurate_retrieval_blocked.sh`; the current default bounded run (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`) completes with `memoryagentbench_overall_score=1.0` and `exact_match=3/3`, and the bounded slice materializes proxy evidence of retrieval participation for every bounded case (`retrieval_answer_cases=3`, `fallback_scan_cases=0`). Retrieval relevance proxy is green across the slice (`relevant_retrieval_evidence_cases=3`, `top_ranked_relevant_retrieval_cases=3/3`), and the current bounded run also materializes lexical/structural proxy support signals `gold_answer_supported_retrieval_cases=3/3`, `top_ranked_gold_answer_supported_retrieval_cases=3/3`, `top_ranked_relevance_and_gold_answer_supported_retrieval_cases=3/3`, plus a stronger top-ranked `anchored_fact_shape_proxy` layer with `top_ranked_structural_fact_supported_cases=3/3`. The key new state change is that benchmark-specific shaping is no longer present in the bounded slice: `benchmark_specific_query_override_cases=0`, `benchmark_specific_window_override_cases=0`, `benchmark_specific_answer_extraction_cases=0`, `benchmark_specific_shaping_present=false`, `generic_runtime_maturity=true`. Query shaping is now generic on this bounded slice, answer extraction is generic, and the shaping boundary is driven by explicit runtime metric flags instead of re-deriving shaping from question text. The runtime now also proves identical-corpus reuse on the current default `3-case` slice: `runtime_corpus_unique_sha_count=1`, `runtime_corpus_reused_cases=2`, reused cases keep `index_project_ms=0`. A fresh profiling-guided runtime fix then corrected the synthetic-runtime edge-cache skip gate so it matches the actual `.md` bounded runtime corpus instead of a non-existent `source_kind=markdown` path: on the current bounded slice this drops cold first-case `index_project_ms` to about `0.87s`, bounded `index_project_ms.avg` to about `0.29s`, and bounded `total_case_ms.avg` to about `0.91s`, while preserving the same bounded proof/truth contract. `latency_maturity=false` still remains honest, but the blocker changed shape: the contour is still not latency-grade because this is only a fixed bounded slice, not because `index_project_ms` still dominates the slice average. The remaining blocker is therefore no longer benchmark-specific shaping, bounded top-rank proxy support, or the old cold-index-dominance story, but semantic relevance maturity, missing benchmark-grade scorer parity, and bounded-only latency evidence. The runtime/restore code path is fail-closed for resume artifacts: `requests.jsonl` must carry non-empty `bench` and `dataset`, and persisted `.case-metrics.jsonl` rows must carry the explicit benchmark-specific shaping flags plus the top-ranked retrieval telemetry fields, otherwise resume/reuse is rejected rather than silently downgrading the shaping boundary. The proof now also re-runs targeted Rust negative tests for those reject paths and for the `paths.txt`/same-hash reuse gate; those tests prove the reject-path contract for malformed identity/reuse artifacts, not a broader filesystem-stability guarantee. `bounded-proof-contract.json` keeps the bounded blocked scope, non-semantic interpretation of retrieval-hit rates, structural-fact proxy ceiling, measured reuse, and non-benchmark-grade maturity machine-readable rather than implicit;
- AMA-Bench keeps this evidence explicitly bounded: prep and small runtime+baseline-score proof are materialized from the manual HF dataset install, but full dataset runtime and benchmark-grade maturity remain separate open work;
- stale runtime attempts under `state/external-benchmarks/memory/**/status.json` are not closure evidence while their stage remains `running`;
- therefore Stage 0-10 stays internal-closed, bounded LongMemEval runtime+baseline-score evidence exists only for the named dataset/limit, but `external benchmark-grade long-term memory maturity` remains `not-fully-materialized` until full dataset runtime+score, semantic retrieval precision and upstream scorer parity are proven.

Consensus records for the external memory benchmark audit:
- `AMAI-AUDIT-EXTMEM-001`
  - `claim_owner`: docs previously risked letting internal Stage 0-10 closure imply `external benchmark-grade long-term memory maturity`;
  - `implementation_verifier`: `scripts/proof_memory_external_benchmarks.sh`, `src/external_benchmark.rs`, `config/external_benchmark_targets.toml`, `config/external_benchmark_datasets.toml`;
  - `proof_owner`: current proof covers source/tool preflight, dataset download, adapter workspace, normalized cases for LongMemEval/MemoryAgentBench/LoCoMo, and one synthetic runtime+score smoke;
  - `consensus_verdict`: `partial`;
  - `required_doc_action`: `split_internal_vs_external_maturity`;
  - `required_implementation_action`: run real benchmark dataset runtime predictions, score those predictions, and add upstream scorer parity before any benchmark-grade maturity claim.
- `AMAI-AUDIT-EXTMEM-002`
  - `claim_owner`: LoCoMo normalized-case readiness must not pass with empty question/context/answer fields;
  - `implementation_verifier`: `normalize_json_record` expands `qa[]`/`qas[]`, preserves scalar/adversarial answers and renders `conversation.session_*` as context;
  - `proof_owner`: `proof_memory_external_benchmarks.sh` validates `locomo10` manifest with non-zero total and zero missing question/context/id/answer;
  - `consensus_verdict`: `verified_working` for normalized-case prep only;
  - `required_doc_action`: `keep` the fix claim but keep full benchmark maturity separated;
  - `required_implementation_action`: extend from normalized cases to real runtime+score and upstream scoring parity.
- `AMAI-AUDIT-EXTMEM-003`
  - `claim_owner`: AMA-Bench must not be implied as normalized/evaluable just because a manual marker or empty placeholder artifacts exist;
  - `implementation_verifier`: `normalize_json_record` expands `qa_pairs[]`, renders `trajectory[]` as context, the shared JSON prep path falls back from full-document parse to JSONL line parsing for multiline object-per-line datasets, bounded prep manifest stats now recompute from written `cases.jsonl` so `limit` stays truthful for multi-QA records, and `prep_validation` now fail-closes on zero cases, missing required fields, duplicate `case_id` values, and invalid normalized case field types/shapes for `bench`, `dataset`, `case_id`, `question`, `context`, `answer`, and `metadata`;
  - `proof_owner`: `proof_memory_external_benchmarks.sh` validates non-empty `ama_bench_manual` normalized cases and a clean manifest from the manual HF dataset install; `proof_memory_external_real_bounded_ama_bench.sh` validates bounded prep, runtime predictions, completed status, retrieval/accounting metrics and baseline score output for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT` cases;
  - `consensus_verdict`: `bounded_real_runtime_score`;
  - `required_doc_action`: `keep_bounded_not_full_maturity`;
  - `required_implementation_action`: scale AMA-Bench from bounded runtime+baseline-score evidence to full dataset runtime and broader benchmark evidence before any benchmark-grade maturity claim.
- `AMAI-AUDIT-EXTMEM-004`
  - `claim_owner`: `external-memory-run` and `external-memory-score` command availability must not be presented as full evaluator maturity;
  - `implementation_verifier`: CLI commands exist and can index a prepared request context, retrieve a candidate answer and baseline-score predictions;
  - `proof_owner`: current proof uses exactly one synthetic exact-match case;
  - `consensus_verdict`: `internal_closed_proof_refresh_required`;
  - `required_doc_action`: `downgrade` any full-evaluator wording to command-contract smoke wording;
  - `required_implementation_action`: run full prepared requests for each materialized external dataset, persist predictions/status/metrics, score real outputs and reconcile with official benchmark scorers where available.
- `AMAI-AUDIT-EXTMEM-005`
  - `claim_owner`: bounded real LongMemEval runtime+score can now be claimed, but only at the named dataset/limit and baseline-scorer scope;
  - `implementation_verifier`: `src/external_benchmark.rs` now retries zero-hit benchmark questions with a relaxed OR query, records retrieval query/attempt metrics, exposes `answer_source_boundary` for retrieval-answer vs fallback-scan accounting, exposes `retrieval_relevance_boundary` for query-overlap relevance proxy accounting, exposes `gold_answer_relevance_boundary` for benchmark-answer lexical support accounting, exposes `official_scorer_boundary` for the LongMemEval upstream scorer contract, exposes `external-memory-official-judge` for official judge execution/log materialization, exposes `external-memory-official-score` for official eval-log reconciliation, allows explicit `external-memory-prepare --source-path` for curated proof samples, and keeps score output fail-closed on `official_upstream_scorer_parity=false` with version-pinned `boundary_version` and explicit `maturity_blocking_reasons`;
  - `proof_owner`: `./scripts/proof_memory_external_real_bounded.sh` validates real `longmemeval_s_cleaned` prep, runtime predictions, completed status, retrieval-backed case metrics, answer-source accounting, query-overlap retrieval relevance accounting, gold-answer lexical support accounting, official scorer contract boundary and baseline score output; `./scripts/proof_memory_external_official_judge.sh` validates official judge no-live, missing-key and non-official-model fail-closed paths; `./scripts/proof_memory_external_official_judge_api_failure.sh` validates fake-provider rate-limit/upstream-error/response-contract/transport fail-closed paths without materializing eval logs; `./scripts/proof_memory_external_official_judge_live_bounded.sh` validates the bounded live-operator path and currently proves fail-closed no-key behavior on the real bounded artifacts, while key-backed runs also invoke the Rust secret verifier across the proof output directory; `./scripts/proof_memory_external_official_judge_live_balanced.sh` validates six official question types, source-path normalization and six Amai runtime predictions before the same no-key/live guard; `./scripts/proof_memory_external_official_score_reconcile.sh` validates official eval-log reconciliation, missing-log and invalid-log fail-closed paths;
  - `consensus_verdict`: `bounded_real_runtime_score`;
  - `required_doc_action`: keep this separate from full external benchmark-grade maturity and from upstream scorer parity;
  - `required_implementation_action`: scale to full prepared datasets, reduce fallback-scan dependence with measured retrieval-answer coverage, replace query-overlap proxy with gold-labeled semantic relevance, and reconcile official upstream scorers before promoting the broader maturity claim.
- `AMAI-AUDIT-EXTMEM-006`
  - `claim_owner`: bounded real MemoryAgentBench runtime+score can now be claimed, but only for the named `memoryagentbench_conflict_resolution`, `memoryagentbench_long_range_understanding`, and `memoryagentbench_test_time_learning` dataset/limit slices and baseline scorer scope;
  - `implementation_verifier`: the shared external-memory runtime/score contour already supports `memoryagentbench_overall_score`, `answer_source_boundary`, `retrieval_relevance_boundary`, `gold_answer_relevance_boundary`, and an `official_scorer_boundary` that stays blocker-visible with `source_kind=official_scorer_contract_unavailable`;
  - `proof_owner`: `./scripts/proof_memory_external_real_bounded_memoryagentbench.sh` validates bounded prep, runtime predictions, completed status, retrieval/accounting metrics, baseline score output, and explicit `runtime_corpus_sha256` / `runtime_corpus_reused_from_previous_case` truth for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT` cases on `memoryagentbench_conflict_resolution`, `memoryagentbench_long_range_understanding`, and `memoryagentbench_test_time_learning`;
  - `consensus_verdict`: `bounded_real_runtime_score`;
  - `required_doc_action`: `keep_bounded_not_full_maturity`;
  - `required_implementation_action`: expand from the bounded `conflict_resolution`, `long_range_understanding`, and `test_time_learning` slices to more MemoryAgentBench datasets and broader real runtime evidence before any benchmark-grade maturity claim.
- `AMAI-AUDIT-EXTMEM-007`
  - `claim_owner`: bounded `memoryagentbench_accurate_retrieval` runtime/score must not be promoted as retrieval-backed bounded maturity just because the 3-case slice completes or because baseline score becomes non-zero;
  - `implementation_verifier`: the bounded slice now materializes prep, runtime completion, generic tight-window runtime shaping, generic bounded answer extraction, generic bounded relaxed-query behavior and perfect bounded baseline score output, with `answer_source_boundary.retrieval_answer_cases=3`, `fallback_scan_cases=0`, `retrieval_relevance_boundary.relevant_retrieval_evidence_cases=3`, `gold_answer_relevance_boundary.gold_answer_supported_retrieval_cases=3`, and `gold_answer_relevance_boundary.top_ranked_gold_answer_supported_retrieval_cases=3`; `benchmark_specific_shaping_boundary` is now clean in the bounded run (`query_override_cases=0`, `window_override_cases=0`, `answer_extraction_cases=0`, `generic_runtime_maturity=true`). The gold-answer/retrieval relevance boundaries still remain lexical/proxy rather than semantic, so the contour is stronger bounded runtime evidence than before but still not semantic maturity;
  - `proof_owner`: `./scripts/proof_memory_external_real_bounded_memoryagentbench_accurate_retrieval_blocked.sh` validates this blocker-visible bounded profile;
  - `consensus_verdict`: `bounded_blocked_profile`;
  - `required_doc_action`: `keep_blocker_visible_not_whitelisted`;
  - `required_implementation_action`: replace lexical/proxy relevance accounting with semantic relevance evidence and establish upstream scorer parity before promoting `accurate_retrieval` from bounded generic-runtime proof into benchmark-grade or fully trusted maturity.

Consensus records for the scientific Queue 4/5 status-truth audit:
- `AMAI-AUDIT-SCI-Q45-001`
  - `claim_owner`: Queue 5 `Poisson capacity` docs may say the forecast-only surface is materialized, but must not freeze a volatile live window as durable truth;
  - `implementation_verifier`: `src/capacity_forecast.rs`, `src/dashboard/dashboard_service_cards.rs::build_capacity_forecast_card`, `scripts/proof_observability.sh`;
  - `proof_owner`: `proof_observability.sh` asserts `capacity_forecast.model_version`, read-only guardrails, project-scoped history, `nats_events` family and allowed `measured|insufficient_sample` window states;
  - `consensus_verdict`: `materialized_read_only_surface`;
  - `required_doc_action`: keep implementation claim, but describe exact `history_points`, `sample_count`, `lambda` and `capacity_margin` values only as dated raw snapshot observations, not as permanent status;
  - `required_implementation_action`: add a stable measured-window validation lane before documenting any durable `5m = measured` or capacity-quality claim.
- `AMAI-AUDIT-SCI-Q45-002`
  - `claim_owner`: Queue 4 `regression explain surface` docs may say the explain surface is production-visible, but must not imply measured regression quality when the sample pool is one-sided;
  - `implementation_verifier`: `src/regression_explain.rs`, `src/dashboard/dashboard_service_cards.rs::build_regression_explain_card`, `scripts/proof_observability.sh`;
  - `proof_owner`: `proof_observability.sh` asserts `regression_explain.model_version`, read-only guardrails, at least three outcomes and allowed `measured|insufficient_sample|not_materialized` statuses;
  - `consensus_verdict`: `materialized_read_only_surface_quality_insufficient`;
  - `required_doc_action`: keep production-visible/read-only claim, but keep measured-quality claim blocked until two-sided samples produce measured model output;
  - `required_implementation_action`: build or wait for a two-sided sample contour and add a measured-quality proof before changing `insufficient_sample` wording.
- `AMAI-AUDIT-SCI-Q45-003`
  - `claim_owner`: Stage 10 `fresh-green` may reference Queue 4/5 dashboard/snapshot guardrails only as bounded observability proof, not as final scientific validation;
  - `implementation_verifier`: `scripts/proof_observability.sh`, `cargo run -- observe snapshot`, dashboard service-card builders;
  - `proof_owner`: current proof checks the cards and raw snapshot contracts, while raw values remain time/window dependent;
  - `consensus_verdict`: `proof_green_for_contract_not_for_measured_quality`;
  - `required_doc_action`: separate `guardrail contract green` from `measured statistical quality achieved`;
  - `required_implementation_action`: preserve fail-closed status rendering and add dedicated measured-quality/capacity validation when enough raw data exists.

Compatibility memory bridge consensus records:
- Fresh proof 2026-04-25: `./scripts/proof_memory_bridge_search.sh` passed after rebuilding release `amai` and `memory`; `cargo test --quiet --bin memory` passed (`11 passed`). The accepted claim remains the release compatibility bridge output contract, not a broader retrieval-quality verdict.
- `AMAI-AUDIT-BRIDGE-001`
  - `claim_owner`: docs say local `memory search` through the Amai compatibility bridge prints hits plus `Почему вошло` and `Почему часть не вошла`;
  - `implementation_verifier`: `src/bin/memory.rs::print_search_state`, `included_reason_line`, `excluded_reason_line`, and `src/onboarding.rs::install_memory_bridge`;
  - `proof_owner`: `./scripts/proof_memory_bridge_search.sh` now rebuilds `target/release/amai` and `target/release/memory` before checking the release bridge output, and targeted `cargo test --bin memory` covers explanation fallback lines; both were rerun green on 2026-04-25;
  - `consensus_verdict`: `verified_working`;
  - `required_doc_action`: keep the bridge explainability claim, but require this proof for future edits to bridge output or release-binary install path;
  - `required_implementation_action`: preserve always-present explanation lines for zero-hit, cache-hit and missing-decision-trace paths.

Operational MCP launcher freshness consensus records:
- Fresh proof 2026-04-25: `./scripts/proof_mcp_launcher_freshness.sh` passed. The proof bootstrapped the stack, shimmed `cargo`, started `scripts/run_mcp_stdio.sh`, completed JSON-RPC initialize and repeated `amai_continuity_startup`, then asserted the launcher chose `cargo run --release --quiet -- mcp serve`. This refresh proves launcher freshness/protocol-clean startup only, not cross-client live-host behavior.
- `AMAI-AUDIT-MCP-LAUNCHER-001`
  - `claim_owner`: docs say MCP startup is correctness-first and should not silently reuse stale `target/release/amai` when source code has changed;
  - `implementation_verifier`: `scripts/run_mcp_stdio.sh`, `scripts/run_mcp_stdio.ps1`, and generated MCP client configs that point to those launchers;
  - `proof_owner`: `./scripts/proof_mcp_launcher_freshness.sh` shims `cargo`, starts the stdio MCP server, performs JSON-RPC initialize plus repeated `amai_continuity_startup`, and asserts the runner used `cargo run --release --quiet -- mcp serve`; rerun green on 2026-04-25;
  - `consensus_verdict`: `verified_working`;
  - `required_doc_action`: keep the cargo-first claim, but state the binary fallback is only a no-cargo degrade path and stdout must stay protocol-clean before the JSON-RPC handshake;
  - `required_implementation_action`: preserve cargo-first launch order and keep startup diagnostics out of stdout for all stdio launchers.

MCP handshake/runtime contract consensus records:
- Fresh proof 2026-04-25: `./scripts/proof_mcp.sh` passed with `proof_scope=full`, `critical=0`, `memory_matrix_tasks_failed=0`, prompts `amai-context-pack`, `amai-continuity-startup`, `amai-onboarding`, and token savings `83.69098712446352%` (`factor=6.131578947368421`). This refresh verifies the MCP handshake/runtime/proof-session contract, not live-client UX.
- `AMAI-AUDIT-MCP-CONTRACT-001`
  - `claim_owner`: docs describe MCP initialize/manifest, prompt list and startup runtime artifact contract for supported clients;
  - `implementation_verifier`: `src/mcp.rs::prompt_definitions`, `src/mcp.rs::verify_mcp`, `src/mcp.rs::protocol_manifest`, `src/token_budget/token_budget_runtime_support.rs::resolve_token_budget_config_path`, `.amai/onboarding/project-chat-startup-contract.json`, `.amai/onboarding/project-chat-startup-agent-contract.json`;
  - `proof_owner`: `./scripts/proof_mcp.sh` validates the prompt set through `prompts/list`, fetches `amai-onboarding` and `amai-continuity-startup`, calls `amai_continuity_startup`, and checks startup summary fields; targeted token-budget config fallback test covers registered project roots without local `config/token_budget_profiles.toml`; contract artifacts currently pin `workspace-startup-runtime-state-v4`; rerun green on 2026-04-25;
  - `consensus_verdict`: `verified_working_with_doc_drift_fixed`;
  - `required_doc_action`: keep MCP prompt/runtime-contract claims, but list all three prompts (`amai-onboarding`, `amai-continuity-startup`, `amai-context-pack`) and keep runtime artifact version at `workspace-startup-runtime-state-v4`;
  - `required_implementation_action`: any future runtime artifact version bump must update `src/mcp.rs`, onboarding contract artifacts, managed startup instructions and docs in one proof-backed change; `amai_continuity_startup` must keep falling back to Amai's own token budget config when the target registered project has no local token-budget profile file.
- `AMAI-AUDIT-MCP-CONTRACT-002`
  - `claim_owner`: `proof_mcp.sh` claims to validate MCP token-ledger turn attach through `amai_observe_whole_cycle_turn`;
  - `implementation_verifier`: `src/mcp.rs::spawn_proof_session`, `src/mcp.rs::run_smoke_proof`, `src/token_budget/token_budget_runtime_event_flow.rs::attach_whole_cycle_observed_to_turn_group`;
  - `proof_owner`: `./scripts/proof_mcp.sh` must run its spawned MCP server with an explicit proof-scoped `CODEX_THREAD_ID` and reuse that same id for turn-scoped assistant-generation attach; rerun green on 2026-04-25;
  - `consensus_verdict`: `verified_working`;
  - `required_doc_action`: document this as an MCP proof hermeticity requirement, not as a live-client behavior guarantee;
  - `required_implementation_action`: keep the proof child process thread-bound to a generated `proof-mcp-thread-*` id so context-pack token events and `amai_observe_whole_cycle_turn` share one observed scope.

Обязательное следующее действие для любой команды, которая проверяет соответствие docs и кода:
1. собрать claim inventory по словам `closed / green / materialized / работает / доказано`;
2. для каждого claim указать code surface, proof surface, dashboard/raw lane и freshness timestamp;
3. downgrade-ить claims без fresh evidence в `proof-refresh-required`;
4. снять checkbox только если fresh proof реально failing или implementation surface отсутствует;
5. после правки пройти `agent_preflight`, `maintainability_gate`, `implementation_status_sync_guard` и записать continuity handoff.

Claim inventory snapshot 2026-04-25 for the current status-truth audit cluster:

| Claim id | Current allowed claim | Code surface | Proof surface | Dashboard/raw lane | Freshness | Verdict / downgrade boundary |
|---|---|---|---|---|---|---|
| `AMAI-AUDIT-CLIENT-001` | compact-chat host launch now distinguishes no-request, no-bridge-command, policy-disabled, opt-in command success/failure and per-client launch support states; non-auto-launch clients also surface truthful client-specific manual fallback guidance instead of a codex-only generic note; dashboard target selector now also surfaces auto-launch bridge status/reason/UX boundary instead of hiding that state behind runtime payload only; real client UX remains not proven. | `src/continuity/continuity_compact_chat_helpers.rs`, `src/continuity.rs`, `src/dashboard/dashboard_client_budget_support.rs`, `src/observe/observe_control_api.rs` | `cargo test --quiet compact_chat`, `cargo test --quiet maybe_launch_compact_chat_host`, `cargo test --quiet compact_chat_notice_kind_preserves_fail_closed_host_states`, `./scripts/proof_client_clean_chat_launch.sh`, helper/API assertions inside compact-chat paths | compact-chat payload / client-budget guard raw payload / dashboard target selector note | 2026-04-30 | `host_state_machine_command_contract`: `available_not_requested`, `bridge_unavailable`, `disabled_by_policy`, `requested`, `launch_failed`, public env-gated wrapper, API notice-kind mapping, VS Code `code chat` command-contract and non-VSCode `manual_only` gap are proof-backed; manual fallback for Codex/Hermes/OpenClaw/generic-style clients is now client-specific and surfaces startup/reconnect assist instead of a codex-only note; dashboard selector now also exposes auto-launch status / unavailable reason / UX boundary from the same continuity truth surface; seamless clean-chat migration and live-client UX are still not proven. |
| `AMAI-AUDIT-CLIENT-002` | Hermes onboarding creates compact startup/profile artifacts and sticky project profile. | `config/client_targets.toml`, `src/onboarding.rs::ensure_hermes_project_profile`, `src/onboarding.rs::remove_hermes_project_profile` | `./scripts/proof_client_reconnect.sh`, `./scripts/proof_remote_onboarding.sh`, targeted Rust onboarding tests | generated `.hermes.md` / managed profile / client config artifacts | 2026-04-25 | Verified for profile/install contract only; full live Hermes agent behavior remains `proof-refresh-required`. |
| `AMAI-AUDIT-CLIENT-003` | reconnect assist is materialized for supported client configs. | `scripts/reconnect_local.sh`, `scripts/cleanup_mcp_orphans.sh`, `scripts/amai_exec.sh`, startup contract reconnect helper fields | `./scripts/proof_client_reconnect.sh`, `./scripts/proof_mcp_orphan_cleanup.sh` | reconnect helper output, orphan MCP cleanup proof and restored startup artifact files | 2026-04-26 | Verified for reconnect assist and orphan cleanup only; seamless host-side clean-chat migration remains open. |
| `AMAI-AUDIT-CLIENT-004` | remote onboarding proof is offline-deterministic through fake SSH and payload-boundary checks. | `scripts/onboard_remote_client.sh`, `scripts/sync_remote_repo.sh`, `scripts/proof_remote_onboarding.sh`, `scripts/proof_remote_repo_sync_payload.sh` | `./scripts/proof_remote_onboarding.sh`, `./scripts/proof_remote_repo_sync_payload.sh` | fake-ssh run artifacts and sync payload manifest | 2026-04-25 | Verified for offline remote onboarding proof; live remote-host availability/e2e remains a separate proof lane. |
| `AMAI-AUDIT-AUTOSTART-001` | local install materializes and enables `systemd --user` `amai-stack.service` for user-manager startup. | `src/onboarding.rs`, `scripts/install_stack_autostart.sh`, `scripts/run_stack_service.sh`, `scripts/bootstrap_stack.sh` | `./scripts/proof_stack_autostart.sh`, `./scripts/proof_bootstrap_volume_dirs.sh`, `./scripts/proof_onboarding.sh` | rendered user unit and `tmp/onboarding/proof-vscode.out` install output | 2026-04-25 | `partial`: does not prove headless reboot, linger, or system-service semantics. |
| `AMAI-AUDIT-BRIDGE-001` | compatibility `memory search` prints hits plus `Почему вошло` and `Почему часть не вошла`. | `src/bin/memory.rs::print_search_state`, `src/onboarding.rs::install_memory_bridge` | `./scripts/proof_memory_bridge_search.sh`, `cargo test --quiet --bin memory` | release bridge stdout from `target/release/memory search` | 2026-04-25 | Verified output/explainability contract only; retrieval relevance quality remains governed by context-pack/retrieval proofs. |
| `AMAI-AUDIT-MCP-LAUNCHER-001` | stdio MCP launcher prefers `cargo run --release --quiet -- mcp serve` when `cargo` exists and keeps stdout protocol-clean. | `scripts/run_mcp_stdio.sh`, `scripts/run_mcp_stdio.ps1`, generated MCP configs | `./scripts/proof_mcp_launcher_freshness.sh` | JSON-RPC initialize plus cargo-shim command trace | 2026-04-25 | Verified shared stdio launcher path only; cross-client live-host behavior remains separate. |
| `AMAI-AUDIT-MCP-CONTRACT-001/002` | MCP handshake/runtime contract exposes the expected prompts, tools, startup artifact shape and proof-scoped token attach. | `src/mcp.rs`, `src/token_budget/token_budget_runtime_support.rs`, `.amai/onboarding/project-chat-startup-contract.json`, `.amai/onboarding/project-chat-startup-agent-contract.json` | `./scripts/proof_mcp.sh` | MCP `prompts/list`, startup runtime artifact, proof-session token lane | 2026-04-25 | Verified MCP runtime/proof-session contract only; live-client UX is not covered. |
| `AMAI-AUDIT-EXTMEM-001..005` | external memory benchmark lane has dataset/source prep, normalized cases for materialized datasets, synthetic command-contract smoke, bounded LongMemEval and AMA-Bench runtime+baseline-score evidence, answer-source/query-overlap/gold-answer support accounting, official scorer contract boundary, official judge execution/log fail-closed lane, bounded and six-type live-operator guards and official eval-log reconciliation. | `src/external_benchmark.rs`, `config/external_benchmark_targets.toml`, `config/external_benchmark_datasets.toml`, `docs/MEMORY_BENCH_RUNBOOK.md` | `./scripts/proof_memory_external_benchmarks.sh`, `./scripts/proof_memory_external_real_bounded.sh`, `./scripts/proof_memory_external_real_bounded_ama_bench.sh`, `./scripts/proof_memory_external_official_judge.sh`, `./scripts/proof_memory_external_official_judge_api_failure.sh`, `./scripts/proof_memory_external_official_judge_live_bounded.sh`, `./scripts/proof_memory_external_official_judge_live_balanced.sh`, `./scripts/proof_memory_external_official_score_reconcile.sh` | `state/external-benchmarks/memory/**/latest/{cases,requests,manifest}.json*`, synthetic predictions/score output, `tmp/external-memory-real-bounded/**/{predictions,status,score,metrics,official-live-judge-summary}.json*`, `tmp/external-memory-official-live-balanced/**`, `tmp/external-memory-official-judge/**`, `tmp/external-memory-official-score-reconcile/**` | 2026-04-27 | `bounded_real_runtime_score`: bounded real LongMemEval and AMA-Bench proofs are materialized only for the named datasets/limits; `answer_source_boundary` makes fallback-scan dependence visible; `retrieval_relevance_boundary` is query-overlap proxy only; `gold_answer_relevance_boundary` is benchmark-answer lexical support only; LongMemEval `official_scorer_boundary` records the upstream scorer contract, while AMA-Bench keeps `official_scorer_boundary.source_kind=official_scorer_contract_unavailable`; `external-memory-official-judge` embeds prompt templates and proves fail-closed no-live/missing-key/non-official-model behavior; fake-provider API-failure proof validates rate-limit/upstream-error/response-contract/transport blocked summaries without eval-log materialization; bounded live-operator proof currently proves missing-key fail-closed behavior on real bounded artifacts without writing eval-results and key-backed runs add Rust-native proof output secret scanning; six-type live-operator proof covers every official LongMemEval question type in a bounded curated sample and currently proves the same no-key blocker; `external-memory-official-score` reconciles upstream-style eval logs; full benchmark-grade maturity remains blocked on an authorized real live judge run, full dataset runtime+score, gold-labeled semantic retrieval precision and upstream scorer parity. |

No checkbox is removed by this snapshot: current audit found downgraded broad claims and open proof lanes, but not a fresh failing proof or absent implementation surface for the internal Stage 0-10 checkboxes.

## Готовые механизмы проверки по этапам

Это уже существующие рабочие механизмы проекта.

Их смысл:
- агент видит их прямо рядом с этапами;
- не догадывается по именам;
- не ищет по всему `scripts/`;
- берёт сначала готовый harness, а не выдумывает локальную проверку.

### Этап 0. Общая модель memory fabric

Использовать:
- ручную cross-doc review;
- `git diff`;
- `./scripts/proof_agent_preflight.sh`
- `./scripts/proof_app_db_role_read_only.sh`
- `./scripts/proof_offline_no_run_build.sh`
- `./scripts/proof_nats_auth_render.sh`
- `./scripts/proof_security_hardening_contract.sh`
- `./scripts/proof_ops_security_defaults.sh`
- `./scripts/proof_repo_hygiene_guard.sh`
- `./scripts/proof_maintainability_gate.sh`
- `./scripts/proof_maintainability_stage_close_guard.sh`
- `./scripts/proof_implementation_status_sync_guard.sh`
- continuity handoff после документных правок.

### Этап 1. Scope и identity control plane

Использовать:
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
- dashboard-check через `/api/dashboard` для четырёх retrieval benchmark-card;
- если `benchmark_qdrant` на dashboard = `null`, брать raw-result из external harvest вместо несуществующей карточки.

### Этап 2. Typed memory envelope + provenance

Текущий честный статус:
- этап закрыт;
- literal envelope/provenance contract materialized first-class в PostgreSQL;
- Stage-2 proof bundle снова зелёный после выделения отдельного stable setup contour без service restarts.

Что уже materialized в рамках этапа:
- `ami.memory_items` расширен до полного typed envelope c `owner_agent_id`, `sensitivity_class`, `trust_state`, `source_event_ids`, `artifact_refs`, `message_refs`, `evidence_span`, `derivation_kind`, `ingest_seq`, `object_version`, `causation_id`, `correlation_id`, `utility_score`, `freshness_score`, `retention_class`, `ttl`, `imported_from`, `schema_version`;
- `ami.memory_provenance` теперь несёт first-class `message_refs`, `evidence_span`, `derivation_kind`, `schema_version`;
- `memory_provenance.details.write_pipeline` теперь materialize-ит общий write-path как machine-readable contour (`raw_event_append`, `memory_candidate_extraction`, `policy_and_scope_filter`, `verification_conflict_check`, `truth_write`, `async_indexing`, `cache_invalidation_fan_out`);
- `ami.memory_envelopes` materialized как canonical typed contract view для envelope/payload reads;
- `ami.retrieval_traces` больше не пустой stage-2 placeholder: durable context-pack path теперь пишет `candidate_summary`, `rerank_summary`, `evidence_sufficiency`, `final_decision`;
- retrieval `decision_trace` materialized как явный read-pipeline contour с `intent_classifier`, `scope_resolver`, `candidate_generation`, `rerank_legality_relevance`, `evidence_ladder`, `escalate_if_needed`, `abstain_if_insufficient` и honest `final_decision`;
- retrieval read-path теперь дополнительно materialize-ит heuristic Stage-2 `intent_classifier` (`continuity / factual_recall / procedural_recall / policy_check / artifact_lookup`), grouped `candidate_generation` surface (`exact / lexical / graph / vector / temporal`) и дублирует `cheapest_sufficient_layer` в `evidence_sufficiency_check` для честного durable trace persistence;
- `verified_write_back` теперь fail-closed: truth-layer требует verified states, non-empty evidence, explicit `metadata.writeback_evidence` и raw/artifact/log/temporal confirmation вместо summary-only write-back;
- proof contour теперь явно проверяет post-stage guarantees: source lineage и temporal truth не теряются, `current / superseded / retracted / unverified` различаются как разные runtime states, retrieval умеет спускаться `summary -> structured -> raw`, а `verified_write_back` без evidence escalation остаётся fail-closed;
- truth-layer surface для roadmap-списка теперь канонизирован жёстче: в PostgreSQL materialized exact alias `ami.project_links` и machine-readable registry `ami.truth_layer_surface_registry`, так что `workspace / project / project_link / memory_item / memory_edge / memory_conflict / memory_provenance / skill_card / policy_rule / retrieval_trace / restore_pack / import_packet / quarantine_item` теперь сверяются через один SQL registry surface, а `memory_relation_edges` и `access_policies` явно помечены как adjunct/control-plane contours, а не скрытые конкуренты canonical truth list;
- добавлен machine-readable guard `./scripts/truth_layer_surface_guard.sh`, который fail-closed проверяет existence exact canonical surfaces и coverage всех roadmap truth entities.
- `trg_ami_memory_items_touch_envelope` и ingest sequence держат temporal ordering / versioning contract;
- write-path для `memory_relation_edges` теперь несёт Stage-2 preflight (`policy_and_scope_filter`, `verification_conflict_check`) и пишет `stage2_runtime` в `evidence_span`;
- write-path для `memory_link_decisions` и `pending_link_proposals` теперь несёт Stage-2 preflight (`policy_and_scope_filter`, `verification_conflict_check`) и пишет `stage2_runtime` в `evidence_span`;
- dedicated Stage-2 setup/proof contour materialized через `./scripts/proof_stage2_setup.sh`, `./scripts/typed_memory_envelope_guard.sh --json`, `./scripts/proof_typed_memory_envelope_contract.sh`.

Использовать:
- `./scripts/typed_memory_envelope_guard.sh --json`
- `./scripts/proof_typed_memory_envelope_contract.sh`
- `./scripts/proof_context_decision_trace.sh`
- `./scripts/proof_working_state_decision_trace.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_observability.sh`

### Этап 3. Commitment / task graph

Использовать:
- `./scripts/proof_execctl_pending_return.sh`
- `./scripts/proof_execctl_restore_stress.sh`
- `./scripts/proof_execctl_resolved_task_ids.sh`
- `./scripts/proof_execctl_resolved_task_identity.sh`
- `./scripts/proof_commitment_task_graph_integrity.sh`

Текущий честный статус:
- stage-local duplicate/resume defects, которые вскрылись ручной проверкой, исправлены;
- `task_node` больше не допускает duplicate `task_key` в пределах одного `project + namespace`, даже если старая линия уже не `hot`;
- `create_task_event` теперь materialize-ит `resumed / reopened / closed / archived` обратно в current `ami.task_nodes`, а не оставляет это только в append-only `ami.task_events`;
- `create_memory_link_decision` теперь materialize-ит graph-effect для `continue / child / new`, а не остаётся только explainability-record:
  - `continue` поднимает candidate-ветку через `continued / resumed`;
  - `child` репэрентит incoming node под candidate и обновляет parent rollups;
  - `new` отцепляет incoming line в самостоятельную workline;
- ambiguity contour больше не рвётся на write-side:
  - `abstain / escalate` уже materialize-ятся как `state_change / evidence_request`;
  - `decision_outcome = pending_link_proposal` теперь тоже materialize-ится в `ami.task_events` как `evidence_request` с `pending_link_ttl_epoch_ms` и `additional_evidence_request`;
  - `pending_link_proposal` в `memory_link_decision` теперь fail-closed без `decision_reason`;
- `create_pending_link_proposal` теперь materialize-ит `evidence_request` в `ami.task_events`, чтобы low-confidence routing не висел только в отдельной truth-table без task-graph следа;
- dedicated Stage 3 proof surface `./scripts/proof_commitment_task_graph_integrity.sh` materialized под roadmap-checklist:
  - система не теряет линии;
  - не плодит лишние дубли;
  - поднимает старую ветку, если это реально она;
  - видит `open / closed / archive` честно.
  - low-confidence routing тоже зафиксирован в bundle:
    - `abstain` обязан писать `state_change` на task-graph слой;
    - `escalate` обязан писать `evidence_request` на task-graph слой с точным `additional_evidence_request`;
    - `pending_link_proposal` через `memory_link_decision` обязан писать `evidence_request` на task-graph слой с `pending_link_ttl_epoch_ms` и `additional_evidence_request`.
- Stage 3 mandatory proof bundle снова целиком зелёный:
  - `./scripts/proof_execctl_pending_return.sh`
  - `./scripts/proof_execctl_restore_stress.sh`
  - `./scripts/proof_execctl_resolved_task_ids.sh`
  - `./scripts/proof_execctl_resolved_task_identity.sh`
  - `./scripts/proof_commitment_task_graph_integrity.sh`
- startup/runtime slice для multi-task obligations ужесточён:
  - `continuity_startup_summary` и `workspace-startup-runtime-state-v4` теперь больше не имеют права
    silently терять `required_task_set` между `chat_start_restore`, summary и startup audit;
  - startup runtime audit теперь fail-closed проверяет не только `required_return_task`, но и
    согласованность `required_task_set_count / required_task_set_present /
    must_preserve_required_task_set`;
  - negative-path regression tests теперь фиксируют, что ложный `required_task_set_count` или
    полностью пропавший `required_task_set` краснят startup runtime state вместо тихого drift.
  - companion startup contract / onboarding / proof surfaces тоже ужесточены:
    `required_task_set` и `required_task_set_summary` теперь обязаны присутствовать в
    `required_summary_fields` и `restored_obligations`, generated startup instructions больше не
    имеют права умалчивать multi-task restore contour, а `proof_onboarding /
    proof_client_lifecycle / proof_art_continuity_startup` теперь ловят такой drift явно.
  - human/prompt surface синхронизирован как projection слой: CLI может показать
    `required_task_set` человеку отдельной строкой, но truth остаётся в machine-readable restore,
    runtime artifact и startup audit.
  - `execctl_resume_obligation` projection path тоже ужесточён: если contract object уже есть,
    но внутри него отсутствует `required_task_set_count`, surface больше не подставляет ложный `0`
    и оставляет `null`, чтобы missing field не маскировался под empty state.
  - человекочитаемый `execctl_resume_contract_summary` тоже перестал лгать о пустоте:
    если `pending_return_count` отсутствует, summary показывает `?`, а не `0`;
    если `required_task_set_count` пропал, но сам `required_task_set` ещё есть,
    summary восстанавливает счётчик по массиву и не теряет multi-task смысл.
  - `startup_runtime_state` CLI-аудит больше не рисует ложный `false`, когда bool-поле аудита
    вообще отсутствует: human output теперь показывает `n/a`, а не маскирует missing state под
    отрицательный verdict. Это же правило теперь покрывает и строку про `startup_contract_sha`,
    так что отсутствие значения не выглядит как “контракт точно не совпал”.
  - обычный human startup summary тоже перестал тихо дорисовывать `0`, когда continuity counters
    отсутствуют: `documents_imported`, `rendered_transcript_files` и `thread_count` теперь
    показывают `n/a`, а `session_memory_files` больше не теряется молча как “как будто bridge-notes нет”.
    То же правило теперь применяется и к `thread_count` в human `chat_start_restore` выводе:
    отсутствие temporal-index count больше не выглядит как реальный `0`.
  - prompt-summary для `execctl_resume_obligation` тоже выправлен: если `resume_state` или
    `pending_return_count` не surfaced, compact restore prompt больше не сочиняет `clear(1)`,
    а показывает честный `n/a(?)` до тех пор, пока точные значения не materialized.
  - display surfaces `client-budget-target` и `compact-chat` тоже перестали тихо подставлять
    пустой `repo_root` или служебный fallback namespace-код: в human output такие поля теперь
    показывают `ещё нет данных`, если payload их реально не surfaced.
  - тем же sweep-проходом дочищены `rotate-chat` и human `chat_start_restore`: пустые
    `last_request`, `client_limits`, `local_path`, `headline` и `next_step` больше не выглядят
    как валидный текст, а честно surfaced как `ещё нет данных`.
  - тем же display/projection sweep-пакетом выправлены и `continuity_answer` JSON surfaces:
    пустые `handoff_summary.headline/next_step` теперь нормализуются в `ещё нет данных`,
    а пустые `chat_lookup.summary_headline/summary_next_step` и такие же поля во
    `selected_time_slice` больше не притворяются осмысленным текстом и surfaced как `null`.
  - этот же закон теперь проведён и по соседним continuity surfaces: `continuity_restore`,
    прямой `render_direct_answer` и `latest_handoff_summary` больше не пропускают пустые
    `headline/next_step` как “валидный текст”, а нормализуют их в `ещё нет данных`.
  - тот же sweep добрался и до `continuity_thread_index` snapshot projection:
    пустые `summary_headline/summary_next_step` теперь не пишутся в observability snapshot
    как “как будто summary есть”, а нормализуются в `null`.
  - в `continuity_import` больше не используется пустая строка как псевдопуть для
    `active_workline_file`: когда файла нет, snapshot и startup import-surface держат
    это состояние как `null/отсутствует`, а не как `""`.
  - startup human/prompt surfaces тоже выровнены по тому же правилу:
    `bootstrap_file`, `latest_rendered_transcript` и blank `handoff headline/next_step`
    больше не проходят через сырые fallback-строки, а нормализуются единообразно как
    `ещё нет данных` во всём `chat_start_restore/startup` слое.
  - в `continuity_thread_index` тот же закон теперь покрывает и path-поля:
    пустые `rendered_transcript`, `source_rollout` и `raw_rollout` больше не пишутся
    как `""`, а нормализуются в `null`.
  - тем же sweep-проходом выровнены и optional text поля `continuity_thread_index` snapshot:
    пустые `title`, `first_user_message`, `last_user_message` и `last_assistant_message`
    больше не сериализуются как осмысленные пустые строки, а нормализуются в `null`.
  - markdown continuity snapshot больше не запускает write→parse drift для пустого
    `rendered_transcript`: пустой путь не печатается как будто это реальный артефакт,
    а parser bootstrap-слоя дополнительно игнорирует blank `rendered_transcript` entries
    вместо того, чтобы затаскивать их обратно как “валидный путь”.
  - тот же fail-closed sweep дошёл и до structured `canonical_eval` details:
    missing `headline/current_goal/next_step/authoritative_event_id/summary` в
    `continuity_startup`, `continuity_restore` и соседних eval-probe payload больше не
    маскируются как `""`, а surfaced как `null`; рядом обновлены stale-тесты, которые
    ждали лишний `recovered_useful` без полного `workspace_restore_pack`.
  - рядом выровнен и startup/verification projection слой:
    `continuity_startup.handoff_summary` теперь нормализует blank `headline/next_step`
    так же, как соседние continuity surfaces, а replay/verification probes больше не пишут
    `selected_headline: ""` там, где по смыслу должен быть либо реальный headline, либо
    нормализованное отсутствие значения.
  - верхний `continuity_verification` payload тоже перестал расходиться с этим законом:
    его `handoff_summary` теперь проходит через тот же canonical normalizer, а
    `working_state_restore.current_goal/next_step/state_lineage.authoritative_event_id`
    больше не вылезают как пустые строки в machine-readable verification snapshot.
  - такой же drift убран и из верхнего `continuity_restore` payload:
    top-level `working_state_restore` теперь нормализуется тем же helper-контуром,
    поэтому blank `current_goal/next_step/authoritative_event_id` больше не проходят
    наружу как осмысленные пустые строки.
  - тот же helper теперь проведён и в `client_budget_target_update`:
    этот structured surface больше не тащит сырой `working_state_restore` наверх,
    а использует ту же blank-to-null нормализацию, что уже применена в
    `continuity_verification` и `continuity_restore`.
  - такой же top-level drift убран и из `continuity_startup`:
    верхний `working_state_restore` теперь тоже идёт через общий normalizer, поэтому
    blank `current_goal/next_step/authoritative_event_id` больше не расходятся между
    `startup`, `restore`, `verification` и соседними structured surfaces.
  - дополнительно выровнен и сам source path для `handoff_summary` в startup context:
    fallback через `active_workline_summary.details` теперь сразу проходит через
    canonical handoff normalizer, а не разносит raw blank `headline/next_step` дальше
    по downstream startup surfaces.
  - top-level `workspace_restore_pack` теперь тоже выровнен по тому же закону:
    в `continuity_startup` и `continuity_restore` blank `summary` больше не проходит
    наружу как пустая строка, а нормализуется через общий projection helper.
  - тот же canonical handoff normalizer теперь применён и в `continuity_import`:
    `active_workline_summary.details` больше не тащит raw blank
    `headline/next_step`, а сохраняет честное projection-состояние
    `ещё нет данных`, если текста реально нет.
  - тот же принцип теперь доведён и до `startup_runtime_state` artifact:
    compact `chat_start_restore.headline/next_step` больше не копируются сырым
    `clone()`-путём и не сохраняют blank text как будто это нормальный restore
    summary; пустое значение теперь остаётся `null`.
  - continuity-answer temporal replay теперь тоже не принимает whitespace-only
    `summary_headline/summary_next_step` за реальные данные: human answer surface
    честно отбрасывает такие псевдо-summary и падает обратно на реальный текст
    сообщения/next-step fallback.
  - startup human summaries теперь так же не считают whitespace-only значения
    реальными данными: `last_request`, `client_limits`, `recent_actions` и
    `active_files` больше не протекают в display/prompt layer как пустой шум.
  - тот же whitespace-only drift убран и из rotate/compact/startup prompt surfaces:
    advisory `last_request`, `client_limits`, `note`, `current_goal` и сводочные
    startup summary-поля больше не попадают в human/prompt output как будто это
    реальные данные, если там только пробелы.
  - execctl startup/resume summaries теперь тоже не считают whitespace-only
    `active_task_headline`, `required_return_headline`, `required_return_next_step`
    и `required_task_set_summary` реальными данными: default/fallback startup
    action больше не строится на таком пустом шуме.
  - соседний prompt/display drift тоже закрыт: whitespace-only blocked-reply
    template и whitespace-only `startup_next_action.headline/required_task_set_summary`
    больше не surface-ятся в rotate/startup prompt как будто там есть осмысленный
    текст.
  - startup restore bundle тоже дочищен: whitespace-only
    `startup_next_action_summary`, `execctl_active_lease_summary`,
    `project_task_tree_summary`, `project_task_ledger_summary`,
    `required_task_set_summary` и fallback `required_return_task.headline`
    больше не surface-ятся как реальные summary-данные.
  - startup runtime gate больше не сочиняет спокойный дефолт, когда самого
    `startup_next_action` нет: вместо ложных
    `action_kind=continue_active_workline`, `blocking=false`,
    `must_follow_startup_next_action=false` и `unrelated_work_allowed=true`
    artifact теперь честно сохраняет `null`, то есть "данных нет".
  - ещё один `canonical_eval`-хвост дочищен в startup/restore ветках:
    `workspace_restore_pack.procedural_surface` больше не становится fake text
    `missing`, если `materialized_surface` реально отсутствует; теперь и там
    сохраняется честный `null`.
  - соседний `canonical_eval` drift тоже закрыт: если `working_state_restore`
    узел вообще не поднят, probe больше не сочиняет строку `missing` для
    `restore_confidence`; теперь отсутствие restore-узла даёт честный `null`,
    а реальное значение `missing/high/...` сохраняется только если оно пришло
    из самих данных.
  - prompt-related projection тоже выровнен: `canonical_eval` больше не
    сочиняет `prompt_length = 0`, если prompt-текста нет вообще, а prompt
    renderer больше не таскает внутрь себя ложный `thread_count=0`, который
    по смыслу там не использовался.
  - top-level `chat_start_restore.restore_confidence` тоже выровнен: слово
    `preliminary` больше не возникает просто из-за отсутствия restore-узла;
    теперь оно остаётся только если реально пришло из данных, а пустой
    no-restore path даёт честный `null`.
  - ещё один source-of-truth хвост закрыт: если `continuity_source_mode` вообще
    отсутствует, startup/eval больше не сочиняют `scoped_import`; теперь
    machine-readable payload даёт честный `null`, а human summary пишет
    `ещё нет данных` вместо выдуманного происхождения.
  - соседнее поле происхождения тоже выровнено: если самого
    `continuity_source_mode` нет, `continuity_source_namespace_code` больше не
    подставляется как текущий namespace; теперь и оно остаётся `null`, а не
    создаёт ложную уверенность в источнике данных.
  - human compact-chat вывод тоже дочищен: если `startup_instruction_mode`
    отсутствует, оператор больше не видит fake label `unknown`, а получает
    честное `ещё нет данных`.
  - соседняя human compact-chat заглушка тоже убрана: если `prompt_text` для
    compact restore отсутствует, вывод больше не притворяется содержимым
    `CHAT_START_RESTORE ...`, а честно пишет `ещё нет данных`.
  - runtime fallback для compact-chat тоже выровнен: если startup runtime
    artifact не принёс `project_code` или `namespace_code`, система больше не
    сочиняет `unknown`/`continuity`, не печатает в human output fake
    `Amai (amai)` и не строит startup command из выдуманного scope; вместо
    этого остаются честные `null` и `ещё нет данных`.
  - соседний human output `client-budget-target` тоже выровнен: если project
    scope в payload отсутствует, вывод больше не подставляет fake
    `Amai (amai)`, а показывает честное `ещё нет данных (ещё нет данных)` через
    общий helper форматирования project-scope.
  - человекочитаемый `client_budget_guard` status тоже выровнен: если у guard
    нет реального `status_label`, ни `rotate-chat`, ни `client-budget-target`
    больше не сочиняют advisory-текст вроде `сожми текущий чат`; теперь human
    output честно пишет `ещё нет данных`.
  - соседний fallback headline тоже выровнен: если и `restore.current_goal`, и
    `handoff.headline` отсутствуют или являются placeholder-текстом, `compact`
    и `rotate` больше не сочиняют рабочую линию `Продолжить активную рабочую
    линию`; теперь там остаётся честное `ещё нет данных`.
  - machine-readable bootstrap summary тоже выровнен: если rendered transcript
    отсутствует, `latest_rendered_transcript` больше не заполняется текстом
    `ещё нет данных` как будто это реальное значение, а сохраняется как честный
    `null`.
  - human compact-chat тоже выровнен по reply-prefix: если `reply_prefix` в
    payload отсутствует или пуст, оператор больше не видит fake KPI-строку
    `5ч KPI: н/д`, а получает честное `ещё нет данных`.
  - prompt-level `execctl` summary тоже выровнен: если `resume_state`
    отсутствует, prompt больше не сочиняет псевдо-состояние `n/a`, а использует
    честный neutral marker `?`, не выдавая это за реальное runtime state.
  - synthetic runtime fallback тоже выровнен: если и `handoff`, и `restore`
    не дают meaningful headline/next_step, machine-readable continuity больше не
    подставляет текст `ещё нет данных` внутрь `active_workline_summary.details`,
    а сохраняет там честные `null`.
  - соседний helper-level `next_step` fallback тоже выровнен: `compact` и
    `rotate` больше не сочиняют `продолжить работу ...` как обязательный шаг,
    если meaningful `next_step` нет ни в `restore`, ни в `handoff`; теперь там
    остаётся честное `ещё нет данных`.
  - human startup/runtime diagnostics тоже выровнены по языку и смыслу: если
    текстовые поля отсутствуют, `startup` больше не печатает `bridge-notes: n/a`,
    а `startup runtime state` больше не показывает `resume_state/action_kind/lease_owner_state`
    как `n/a`; теперь в этих user-facing местах пишется честное
    `ещё нет данных`.
  - prompt-level `execctl_resume_contract` summary теперь тоже не принимает
    whitespace-only `resume_state`, `required_return_headline` и
    `required_return_next_step`: если там нет реального текста, summary не
    делает вид, что это валидное resume-сообщение.
  - startup runtime gate тоже выровнен по fail-closed логике: если
    `chat_start_restore.execctl_resume_state` или
    `required_return_task.headline/next_step` отсутствуют либо состоят только из
    пробелов, `startup_execution_gate` больше не подставляет ложный `clear` или
    пустые строки, а сохраняет `null`, чтобы отсутствие данных не маскировалось
    под “всё чисто”.
  - тот же drift убран и из верхнего startup surface: structured
    `execctl_resume_obligation`, top-level `chat_start_restore.execctl_resume_state`
    и startup prompt больше не подставляют `clear`, если `resume_state`
    отсутствует или состоит только из пробелов; теперь такое состояние
    сохраняется как `null` и prompt не делает вид, что возврат “чисто закрыт”.
  - соседний no-restore fallback тоже выровнен: если самого
    `execctl_resume_contract` нет, `chat_start_restore` и startup prompt больше
    не сочиняют `execctl_resume_obligation.resume_state = clear`; теперь в этом
    контуре сохраняется честный `null`, чтобы отсутствие restore-contract не
    маскировалось под “возврат точно чист”.
  - исправлен front-door drift для `continuity handoff`: если локальный
    `observe /api/continuity-handoff` недоступен, `./scripts/continuity_handoff.sh`
    больше не уходит сразу в тяжёлый `amai_exec.sh -> cargo build --release`
    contour и не выглядит как “handoff завис”. Теперь shell front-door сначала
    использует готовый `./target/release/amai continuity handoff`, а в rebuild
    fallback идёт только если release binary реально отсутствует. Добавлен
    regression proof `./scripts/proof_continuity_handoff_frontdoor.sh`, который
    специально ломает API bind и требует быстрый успешный handoff с реальным
    обновлением `state/continuity-imports/amai/live-handoff.md`.
  - соседний `observe` front-door contour тоже materialized безопасно:
    `./scripts/ensure_observe_frontdoor.sh` теперь может быстро поднять
    `observe serve` только когда bind реально отсутствует, а
    `./scripts/proof_observe_frontdoor_autostart.sh` требует не только
    успешный shell `continuity_handoff`, но и проход соседних read-side
    wrappers `client_budget_root_cause.sh` и `client_budget_gate.sh`, чтобы
    front-door не был “зелёным” только для одного handoff-path.
  - базовые shell continuity wrappers тоже перестали молча зависеть от
    `amai_exec.sh` как от единственного пути запуска: `continuity_startup.sh`,
    `continuity_startup_state.sh`, `continuity_restore.sh` и `continuity_answer.sh`
    плюс delivery surfaces `continuity_compact_chat.sh` и
    `continuity_client_budget_target.sh` теперь сначала используют готовый
    `./target/release/amai`, а только потом
    fallback-ят в `amai_exec.sh`; внутренний autolaunch path в
    `client_budget_gate.sh` для `observe ctl-launch` выровнен так же. Отдельно
    исправлен hidden branch в `continuity_handoff.sh`: missing `--details-file`
    больше не срывается в прямой `scripts/amai_exec.sh` path и не падает
    ложным `No such file or directory`, а повторяет каноническую ошибку
    release binary `failed to read ...`. Добавлен
    regression proof `./scripts/proof_continuity_shell_release_fallback.sh`,
    который временно убирает `scripts/amai_exec.sh` и требует успешный
    `startup`, `startup-state`, `restore`, `answer`, `compact-chat` и
    `client-budget-target` через один release binary, плюс успешный shell
    `continuity_handoff` при сломанном API front-door и negative-path handoff
    с отсутствующим `details-file`.
  - соседний front-door payload contract выровнен и для delivery wrappers:
    `continuity_compact_chat.sh` и `continuity_client_budget_target.sh` больше
    не принимают любой non-empty API body как будто это валидный projection.
    Если `/api/client-budget-compact-chat` или `/api/client-budget-target`
    возвращают non-empty, но structurally invalid JSON surface, wrappers теперь
    explicit fail-closed пишут `continuity compact chat: invalid API payload`
    или `continuity client budget target: invalid API payload` и завершаются
    кодом `12`, вместо неявного jq/set-e abort либо `null`-projection. Добавлен
    hostile proof `./scripts/proof_continuity_frontdoor_invalid_payload_fail_closed.sh`,
    который stub-ит front-door и подсовывает malformed payload для обоих
    delivery surfaces.
  - этот же front-door contract теперь дотянут и до semantic-invalid payload:
    `continuity_compact_chat.sh`, `continuity_client_budget_target.sh` и
    `continuity_handoff.sh` больше не считают успехом JSON, где top-level key
    есть, но обязательные вложенные поля отсутствуют или имеют неправильный
    тип. Такие ответы теперь explicit fail-closed surface-ят
    `invalid API payload` с кодом `12`, вместо того чтобы пропустить частичный
    projection как “валидный”. Добавлен hostile proof
    `./scripts/proof_continuity_frontdoor_semantic_invalid_payload_fail_closed.sh`,
    который подсовывает syntactically valid, но structurally/semantically
    неполный payload для всех трёх wrappers.
  - semantic-invalid boundary tightened дальше, а не оставлен на уровне “есть
    объект и несколько полей”: `continuity_compact_chat.sh` теперь режет blank
    `handoff.headline/next_step`, `continuity_client_budget_target.sh` режет
    boundary-invalid и type-invalid `target_percent` вне канонического набора
    `0..100 step 10` и blank operator text, а `continuity_handoff.sh` требует
    не только nested handoff object, но и `status == ok` плюс непустые
    `headline/next_step` и project/namespace codes. Это значит, что front-door
    больше не считает успехом “формально JSON, но по смыслу сломанный” payload
    с пустыми строками, ложным статусом, blank optional-field values,
    отсутствующими core identifiers или неканоническим/неправильного типа
    `target_percent`; hostile proof
    `./scripts/proof_continuity_frontdoor_semantic_invalid_payload_fail_closed.sh`
    теперь доказывает именно эти boundary/optional-field/type/id negative paths,
    включая blank handoff summary, missing/blank `project.code` и
    `namespace.code`, string/out-of-range `target_percent` и false-positive
    `status`.
  - соседний inter-script transition contract тоже теперь доказан отдельно:
    `./scripts/proof_continuity_frontdoor_transition_contract.sh` materialize-ит
    один валидный front-door sequence `compact-chat -> client-budget-target ->
    handoff` и одновременно hostile-доказывает, что payload shape одного шага не
    может быть silently принят следующим шагом как “свой” projection. То есть
    `compact-chat` отвергает `client-budget-target` shape, `client-budget-target`
    отвергает `compact-chat` shape, а `handoff` отвергает `client-budget-target`
    shape, вместо ложного зелёного межоболочечного перехода.
  - state-integrity boundary для этого же sequence теперь тоже materialized:
    `./scripts/proof_continuity_frontdoor_state_integrity.sh` доказывает, что
    при валидных `compact-chat` и `client-budget-target`, но fail-closed
    `continuity_handoff` на invalid API payload, canonical
    `state/continuity-imports/amai/live-handoff.md` не меняется вообще. То есть
    частичный межскриптовый sequence не оставляет Amai в полузаписанном
    continuity state и не маскирует failed handoff как “как будто запись уже
    произошла”.
  - transport-fallback sequence теперь тоже доказан на том же контуре:
    `./scripts/proof_continuity_frontdoor_transport_fallback_sequence.sh`
    materialize-ит валидные `compact-chat` и `client-budget-target` через
    front-door, затем ломает `/api/continuity-handoff` transport-level exit code
    (`curl` failure path) и доказывает, что shell `continuity_handoff.sh`
    уходит в быстрый local release fallback, успевает записать canonical
    handoff и не оставляет sequence в подвешенном состоянии. То есть transport
    failure на последнем шаге теперь доказан как recoverable continuity path, а
    не как тихая потеря handoff.
  - transport failure matrix для handoff fallback тоже теперь прикрыт:
    `./scripts/proof_continuity_handoff_transport_failure_matrix.sh` прогоняет
    как минимум `curl` exit `7`, `28` и `56` на `/api/continuity-handoff` и
    доказывает, что shell `continuity_handoff.sh` не завязан на один timeout
    case, а уходит в тот же быстрый local release fallback на нескольких
    реальных transport failure ветках, каждый раз записывая корректный canonical
    handoff в bounded latency budget.
  - success-path isolation для этого же handoff contour теперь тоже отдельным
    proof-ом зафиксирован: `./scripts/proof_continuity_handoff_frontdoor_success_path.sh`
    временно убирает и `target/release/amai`, и `scripts/amai_exec.sh`, оставляя
    только валидный front-door payload, и доказывает, что shell
    `continuity_handoff.sh` успешно завершает API path без неявной зависимости от
    local fallback. Это закрывает риск “fallback механизм тихо вмешивается даже в
    normal success path”.
  - concurrent write race для canonical handoff теперь тоже прикрыт отдельным
    hostile proof `./scripts/proof_continuity_frontdoor_concurrent_handoff_race_condition.sh`:
    несколько одновременных `continuity_handoff.sh` через local fallback должны
    все вернуть валидный JSON, а итоговый `state/continuity-imports/amai/live-handoff.md`
    обязан остаться цельным и соответствовать ровно одному из завершившихся
    writers, без дублированных `headline/next_step` линий и без межпроцессной
    порчи canonical handoff файла.
  - bounded soak для repeated transport-fallback handoff тоже теперь materialized:
    `./scripts/proof_continuity_handoff_transport_fallback_bounded_soak.sh`
    прогоняет `12` подряд shell `continuity_handoff.sh` запусков при сломанном
    `/api/continuity-handoff` transport path (`curl exit 28`), требует валидный
    JSON и fresh canonical handoff на каждой итерации, не допускает drift в
    `live-handoff.md`, и дополнительно держит bounded latency contract
    (`max_single_ms <= 5000`, `total_ms <= 25000`). Это уже не разовая fallback
    демонстрация, а proof, что repeated recoverable handoff path остаётся
    устойчивым и не расползается под короткой серией последовательной нагрузки.
  - short burst transport-failure layer теперь тоже материализован:
    `./scripts/proof_continuity_handoff_transport_failure_burst.sh` поднимает
    `8` одновременных `continuity_handoff.sh` при принудительном front-door
    `curl exit 56`, требует валидный JSON от каждого процесса, bounded total
    latency и целостный canonical `live-handoff.md` без дублированных
    `headline/next_step` линий. Это закрывает не только serial soak, но и
    короткий burst fallback path под одновременным transport failure давлением.
  - restart-like idempotency boundary для canonical handoff тоже теперь
    зафиксирован: `./scripts/proof_continuity_handoff_restart_idempotency.sh`
    повторяет один и тот же `continuity_handoff.sh` input дважды через тот же
    fallback-capable shell path и доказывает, что canonical `live-handoff.md`
    остаётся single-entry и не меняет semantic payload для идентичного replay.
    Это не crash-safe durability proof, но уже закрывает важный restart-like
    хвост: identical replay не плодит дубликаты и не искажает последний
    canonical handoff.
  - отдельно materialized и более глубокий state-side-effect слой той же
    идемпотентности: `./scripts/proof_continuity_handoff_state_side_effect_idempotency.sh`
    повторяет идентичный `continuity handoff` на одном и том же
    `project/namespace/agent_scope/thread` и доказывает, что replay больше не
    создаёт новый `authoritative_event_id`, не раздувает
    `continuity_handoff`/`working_state_event` snapshots, не плодит ledger/history
    записи и не меняет semantic restore state. Разрешён только lease heartbeat
    refresh, а не новый смысловой handoff.
  - после этого был найден и закрыт ещё более глубокий resource-growth хвост:
    identical replay всё ещё наращивал `working_state_restore` snapshots
    (`1 -> 2 -> 3 -> ...`) из-за того, что persisted restore payload не имел
    стабильного `source_event_id`, а observability insert трактовал каждый
    refresh как новую snapshot-строку.
  - fix materialized в двух местах:
    - persisted `working_state_restore` payload теперь несёт
      `_observability.source_event_id = authoritative_event_id` и
      `source_kind = working_state_restore_runtime`;
    - `insert_observability_snapshot` теперь для `working_state_restore`
      разрешает update-in-place той же mutable snapshot-строки при том же
      `source_event_id` и более новом `captured_at_epoch_ms`, вместо silent row
      growth.
  - новый bounded soak
    `./scripts/proof_continuity_handoff_state_side_effect_bounded_soak.sh`
    прогоняет `12` одинаковых replay подряд и доказывает, что теперь не растут
    ни `continuity_handoff`, ни `working_state_event`, ни
    `working_state_restore`, а lease heartbeat продолжает честно обновляться.
  - отдельный более глубокий runtime fix materialized и на уровне observability:
    persisted `working_state_restore` snapshot теперь получает стабильный
    `_observability.source_event_id = authoritative_event_id`, а insert-path для
    `working_state_restore` допускает только узкое update-in-place той же mutable
    snapshot-строки при том же `source_event_id` и более новом
    `captured_at_epoch_ms`. Это закрывает реальный resource-growth defect, где
    identical replay уже не плодил новые `continuity_handoff` и
    `working_state_event`, но всё ещё раздувал `working_state_restore`
    snapshot-count (`1 -> 2 -> 3 -> ...`).
  - regression закреплён Rust-native runtime test
    `working_state_restore_snapshot_reuses_same_row_for_newer_same_authoritative_event`
    и bounded soak
    `./scripts/proof_continuity_handoff_state_side_effect_bounded_soak.sh`:
    одинаковый replay теперь удерживает `continuity_handoff = 1`,
    `working_state_event = 1`, `working_state_restore = 1`, не искажает restore
    payload и не ломает bounded latency contract.
  - boundary-path coverage для этого же narrow rule тоже materialized:
    - `persisted_restore_snapshot_payload` не проставляет fake
      `_observability.source_event_id`, если `authoritative_event_id` реально нет;
    - `working_state_restore` с тем же `source_event_id`, но без нового
      `captured_at_epoch_ms`, всё равно reuse-ит ту же snapshot-строку, а не
      наращивает count;
    - другой `source_event_id` при том же timestamp по-прежнему создаёт новую
      строку и не схлопывается ошибочно в старую.
  - temporal regression тоже закрыт отдельно: replay со старым
    `captured_at_epoch_ms` и тем же `source_event_id` больше не имеет права
    перетирать более новый `working_state_restore` payload. Он засчитывается как
    replay для observability history, но сохраняет более свежий payload как
    authoritative runtime snapshot.
  - следующий узкий корень тоже materialized, а не оставлен на словах:
    `prepare_observability_payload("working_state_restore", ...)` теперь
    fail-closed режет malformed update payload, если:
    - нет `working_state_restore` object root;
    - нет непустых `project.code` / `namespace.code`;
    - `captured_at_epoch_ms` не integer;
    - `_observability.source_event_id` не совпадает со
      `state_lineage.authoritative_event_id`.
  - concurrent boundary-path тоже прикрыт Rust-native runtime test:
    одновременный newer/older replay одного и того же
    `authoritative_event_id` удерживает single-row `working_state_restore`
    snapshot и оставляет authoritative payload за более новым состоянием, а не
    даёт гонке перетереть его старым replay.
  - partial malformed path и mixed-quality concurrent path тоже отдельно
    закрыты:
    - частично битый payload с живым root, но без `namespace.code`, теперь
      fail-closed отвергается так же жёстко, как и полностью malformed shape;
    - одновременный valid + malformed replay одного и того же
      `authoritative_event_id` больше не может испортить stored row: valid
      payload materialize-ится, malformed ветка падает с ошибкой, а snapshot
      остаётся single-row без ложного replay-growth.
  - combined invariant drift тоже не оставлен серой зоной: payload, который
    одновременно ломает `namespace.code` и `source_event_id /
    authoritative_event_id` agreement, fail-closed отвергается детерминированно,
    а не проходит частично из-за того, что проверка сработала только по одному
    инварианту.
  - same-timestamp collision policy для одного и того же
    `authoritative_event_id` теперь тоже выведена из неявной гонки в явный
    контракт: `working_state_restore` при одинаковом
    `captured_at_epoch_ms` больше не делает silent last-writer-wins. Первый
    authoritative payload сохраняется, последующие same-timestamp divergent
    replays учитываются как `replay_count`, но не имеют права перетирать stored
    payload.
  - update-path atomicity тоже отдельно доказана: malformed
    `update_observability_snapshot_payload` fail-closed ломается до mutation,
    existing row не меняется, `replay_count` не растёт и в snapshot не остаётся
    полу-обновлённое состояние.
  - same-timestamp mixed-quality collision тоже не оставлена серой зоной:
    если у одного `authoritative_event_id` одновременно приходят valid payload и
    malformed same-timestamp payload, valid row materialize-ится как
    authoritative, malformed ветка fail-closed падает, а stored snapshot не
    получает ни overwrite, ни ложный replay-growth.
  - infrastructure-level write failure path для `observability_snapshots` тоже
    теперь доказан отдельными hostile runtime tests:
    - если connection теряется до `working_state_restore` initial insert, новая
      snapshot-строка не materialize-ится вообще;
    - если connection теряется перед update существующей snapshot-строки,
      existing payload и `replay_count` остаются прежними, то есть write failure
      не оставляет полусостояние и не портит already authoritative row.
  - ambiguity layer для исхода insert теперь тоже перестал быть немой:
    `observability insert` получил отдельную test-only error classification с
    различением `before_write` и `outcome_unknown_after_write`, по тому же
    принципу, который уже materialized в `replace_document_index`.
  - это прикрыто отдельными hostile runtime tests:
    - forced `before_write` гарантирует, что строка не появилась вообще;
    - forced `outcome_unknown_after_write` гарантирует, что строка уже
      materialized в базе, даже если caller получил ambiguous failure вместо
      normal success-path.
  - после этого был закрыт следующий caller-side разрыв:
    `refresh_restore_snapshot` и оба force-refresh пути для
    `working_state_restore` больше не обрываются на ambiguous insert, оставляя
    truth в `observability_snapshots` уже обновлённой, а `restore_pack`
    устаревшим. Теперь при `outcome_unknown_after_write` caller восстанавливает
    `snapshot_id` по canonical `event_key` и всё равно materialize-ит
    `workspace_restore_pack`, то есть верхний runtime surface догоняет уже
    состоявшуюся truth-запись вместо silent divergence.
  - это прикрыто отдельным runtime test:
    `force_refresh_restore_snapshot_outcome_unknown_after_write_still_materializes_restore_pack`
    доказывает, что ambiguous insert не рвёт связку
    `working_state_restore snapshot -> restore_pack`.
  - следующий нижний race-layer для `restore_pack` тоже закрыт:
    `create_restore_pack` больше не делает голый `check-then-insert` для одного
    и того же `source_snapshot_id`. Теперь этот путь сериализуется
    PostgreSQL advisory lock-ом по `namespace_id + pack_kind + source_snapshot_id`,
    поэтому два параллельных materialize-вызова не могут quietly наплодить две
    одинаковые canonical restore-pack строки.
  - это прикрыто hostile runtime test
    `create_restore_pack_concurrent_same_source_snapshot_reuses_single_row`:
    он специально растягивает окно между `lookup` и `insert` и потом запускает
    два concurrent `create_restore_pack` на одном snapshot; обе ветки обязаны
    вернуть один и тот же `restore_pack_id`, а count по
    `project/namespace/pack_kind/source_snapshot_id` должен остаться `1`.
  - соседний semantic-conflict слой для того же `source_snapshot_id` тоже больше
    не тихий: если второй `create_restore_pack` несёт уже другой canonical
    payload/headline/summary/evidence, путь теперь режется fail-closed вместо
    молчаливого возврата первой строки.
  - это прикрыто двумя отдельными hostile runtime tests:
    - `create_restore_pack_same_source_snapshot_conflicting_payload_is_rejected`;
    - `create_restore_pack_concurrent_same_source_snapshot_conflicting_payload_preserves_first_row`.
  - success-path contract для точного replay теперь тоже явно доказан:
    одинаковый `workspace_restore_pack` на том же `source_snapshot_id` обязан
    вернуть тот же `restore_pack_id`, не плодить вторую строку и не мутировать payload.
  - это прикрыто runtime test
    `create_restore_pack_exact_replay_reuses_same_row_without_mutation`.
  - DB-native boundary за ложным `source_snapshot_hint.verified_exists` тоже теперь
    доказан отдельно: если caller соврал про существование `source_snapshot_id`,
    app-layer может пропустить lookup, но PostgreSQL `FOREIGN KEY` всё равно режет
    запись fail-closed и не оставляет строки.
  - это прикрыто runtime test
    `create_restore_pack_missing_snapshot_behind_verified_hint_fails_before_write`.
  - соседние DB-native `CHECK` boundaries для `pack_kind` и `derivation_kind`
    тоже теперь доказаны отдельно: если caller подаёт недопустимое enum-значение,
    `create_restore_pack_detailed` обязан вернуть `BeforeWrite`, а truth-row не
    должна появиться вообще.
  - это прикрыто runtime tests:
    - `create_restore_pack_invalid_pack_kind_fails_before_write`;
    - `create_restore_pack_invalid_derivation_kind_fails_before_write`.
  - delete-path identity bypass через `source_snapshot_id` тоже теперь закрыт
    на уровне schema migration: `restore_packs_source_snapshot_id_fkey` переведён
    с `ON DELETE SET NULL` на `ON DELETE RESTRICT`, поэтому canonical
    `workspace_restore_pack` больше нельзя silently orphan-ить удалением
    `observability_snapshot`.
  - это прикрыто runtime test
    `restore_pack_schema_rejects_source_snapshot_delete_while_restore_pack_depends_on_it`,
    который materialize-ит `workspace_restore_pack`, применяет live migration
    block для FK и доказывает `FOREIGN_KEY_VIOLATION` на delete-path с
    сохранением `source_snapshot_id` у canonical row.
  - тот же identity-law теперь materialized и как прямой schema `CHECK`:
    `workspace_restore_pack` больше нельзя держать с `NULL source_snapshot_id`
    даже через raw SQL или будущий app-bypass.
  - это прикрыто runtime test
    `restore_pack_schema_rejects_raw_workspace_restore_pack_without_source_snapshot`,
    который после migration block доказывает `CHECK_VIOLATION` на raw insert.
  - historical dirty orphan rows для этого же инварианта теперь не просто
    запрещаются на будущее, а канонически вычищаются самой migration-логикой
    перед установкой `CHECK`.
  - это прикрыто runtime test
    `restore_pack_workspace_source_snapshot_check_migration_deletes_dirty_orphans_and_is_idempotent`,
    который вставляет raw orphan `workspace_restore_pack` с `NULL source_snapshot_id`,
    прогоняет migration дважды и доказывает, что строка удаляется, а повторный
    прогон остаётся идемпотентным.
  - destructive cleanup этого migration contour теперь доказан и как
    transactional fail-closed path: если migration падает после cleanup шага,
    orphan row не теряется частично, а откатывается вместе со всем batch.
  - это прикрыто runtime test
    `restore_pack_workspace_source_snapshot_check_migration_failure_rolls_back_cleanup`,
  - live `bootstrap_schema()` и `bootstrap_schema_is_current()` для этого же
    source-identity law теперь доказаны не только текстовым drift-guard, но и
    functional-equivalence proof: если schema искусственно деградировать обратно
    до `ON DELETE SET NULL` и убрать conditional `CHECK`, а затем вставить dirty
    orphan `workspace_restore_pack`, bootstrap обязан восстановить оба закона и
    вычистить orphan до passing currentness-state.
  - это прикрыто runtime test
    `bootstrap_schema_restores_restore_pack_source_identity_law_and_cleans_dirty_orphans`,
  - raw read-path для `restore_pack` теперь тоже fail-closed: `get_restore_pack`
    больше не имеет права сериализовать исторически грязный
    `workspace_restore_pack` с `NULL source_snapshot_id` как будто это валидная
    canonical запись.
  - это прикрыто runtime test
    `get_restore_pack_rejects_dirty_workspace_restore_pack_without_source_snapshot`,
  - same-source read/recovery selector для `workspace_restore_pack` теперь
    выровнен с bootstrap dedupe policy: lookup и ambiguity-recovery больше не
    выбирают строку только по `created_at DESC`, а используют тот же canonical
    порядок `captured_at_epoch_ms DESC NULLS LAST, created_at DESC, restore_pack_id DESC`.
  - смысл этого закона явный: recovery/read-path обязан предпочитать наиболее
    свежую и наиболее полно surfaced source-time строку, а `created_at` и
    `restore_pack_id` разрешены только как deterministic fallback tie-breakers,
    а не как замена source-time semantics.
  - это прикрыто runtime test
    `lookup_restore_pack_by_source_snapshot_id_prefers_canonical_newer_source_time_for_dirty_duplicates`
    и companion-case
    `lookup_restore_pack_by_source_snapshot_id_prefers_non_null_source_time_for_dirty_duplicates`,
  - следующий consumer-side gap вокруг `restore_pack` тоже теперь закрыт: для
    non-core writer paths (`client_budget_target_update`,
    `host_current_thread_control_feedback`, `retrieval_context_pack`) truth больше
    не схлопывается в ложный total failure, если primary `working_state_event`
    уже durably записан, а derivative `refresh_restore_snapshot` сломался.
  - вместо generic `Err` эти paths теперь возвращают явный
    `working_state_write_status = degraded_after_primary_write` и пробрасывают
    warning в user-facing payload/JSON surface, чтобы оператор видел честное
    `event persisted, restore refresh degraded`, а не фиктивное «ничего не
    произошло».
  - это прикрыто runtime test
    `record_client_budget_target_event_reports_degraded_refresh_after_primary_write`,
    `record_context_pack_event_reports_degraded_refresh_after_primary_write` и
    unit guard
    `client_budget_host_control_launch_api_summary_preserves_working_state_write_status`,
  - companion outer-surface contract тоже теперь зафиксирован: `working_state_write_status`
    не теряется на summary-first host-control payload и не срезается на
    model-visible context-pack compaction.
  - это прикрыто тестами
    `thread_bound_host_control_feedback_payload_stays_summary_first`,
    `non_thread_host_control_feedback_payload_stays_summary_first` и
    `context_pack_payload_preserves_working_state_write_status_marker`,
  - consumer-side last-mile for these degraded markers тоже теперь закрыт:
    dashboard/observe surfaces, которые раньше смотрели только на
    `chat_notice/operator_notice.message_text`, теперь доклеивают
    `working_state_write_status.warning` в реально показываемый notice/toast text
    вместо того, чтобы silently держать warning только в JSON.
  - это означает, что degraded marker теперь не просто preserved-in-payload, а
    уже реально surfaced оператору на `client_budget_target_update`,
    `host_current_thread_control_launch` и `host_current_thread_control_feedback`
    путях, даже если consumer читает notice-first surface.
  - source-level direct-payload gap для `client_budget_target_update` тоже теперь
    закрыт: producer в `continuity` больше не отдаёт «чистый»
    `operator_notice.message_text` с warning только в соседнем поле.
  - теперь `client_budget_target_update.operator_notice.message_text` сам уже
    доклеивает degraded `working_state_write_status.warning`, а
    `operator_notice` ещё и несёт сам `working_state_write_status`, так что
    summary-first/direct-payload consumers не обязаны ждать downstream
    observe/dashboard enrichment, чтобы увидеть degraded-after-primary-write.
  - companion downstream correction тоже materialized сразу: после source-level
    enrichment `observe_page_api` для `client_budget_target_update` больше не
    доклеивает тот же warning второй раз поверх уже enriched
    `operator_notice.message_text`, а предпочитает source-level notice как
    canonical message и уходит в fallback append только если source-level text
    отсутствует или пустой.
  - companion dashboard correction тоже теперь pinned: UI target/host-control
    consumers больше не используют raw append helper для already-enriched notice
    paths, а предпочитают source notice как canonical message и используют
    warning-only fallback только при пустом source notice.
  - notice-first summary gap у `client_budget_host_control_launch` тоже теперь
    закрыт: API-level `chat_notice` больше не теряет
    `working_state_write_status`, если consumer читает только notice и не
    разбирает sibling `client_budget_host_control_launch`.
  - companion runtime-summary gap тоже теперь закрыт: compact
    `latest_repo_working_state_restore` budget projection больше не выкидывает
    `working_state_write_status` из `recent_actions[*].host_current_thread_control_feedback`,
    если degraded marker уже присутствует в authoritative restore payload.
  - это значит, что summary-first runtime consumers не теряют degraded
    `after_primary_write` signal при compaction working-state history до
    client-budget surfaces.
  - companion `observe` contract proof теперь тоже pinned на том же инварианте:
    existing broad test для compact budget restore больше не проверяет только
    trimming critical fields, а ещё и требует сохранения degraded
    `working_state_write_status` внутри
    `recent_actions[*].host_current_thread_control_feedback`.
  - wrapper preview gap над тем же contour тоже теперь закрыт: broad proof для
    `compact_budget_snapshot_preview_payload` требует, чтобы degraded
    `working_state_write_status` дошёл и через top-level
    `latest_repo_working_state_restore` preview path, а не только через direct
    `compact_working_state_restore_for_budget`.
  - отдельный source-of-truth drift в preview builder тоже теперь закрыт:
    `collect_client_budget_snapshot_from_db` больше не игнорирует
    `latest_repo_restore_override` при materialization поля
    `latest_repo_working_state_restore`, а использует тот же authoritative
    override/payload contour, который уже был выбран для lease-maintenance.
  - это убирает риск тихого stale DB refetch в preview path, когда caller
    специально передал свежий in-memory restore override.
  - sibling summary-first drift в compact-chat API summary тоже теперь закрыт:
    `compact_chat_api_summary` больше не режет `operator_notice.kind`, так что
    compact-chat notice identity не теряется при summary reshaping и остаётся
    симметричной соседним notice-first surfaces.
  - отдельный delivery-surface drift в том же compact-chat contour тоже теперь
    закрыт: observe API больше не пересчитывает `delivery_surface_notice.kind`
    поверх уже materialized `operator_notice.kind`, а предпочитает source notice
    как authoritative и уходит в `host_launch.status` fallback только если
    source kind отсутствует или пустой.
  - аналогичный source-first drift в `client_budget_target_update` path тоже
    теперь закрыт: `observe_page_api` больше не держит отдельный hardcoded
    `chat_notice.kind`, а предпочитает canonical producer-side
    `operator_notice.kind` и уходит в старый literal fallback только если source
    kind отсутствует или пустой.
  - тот же target-update notice contour теперь выровнен и по thread identity:
    continuity payload materialize-ит `operator_notice.thread_id`, а
    `observe_page_api` предпочитает source thread-id для `chat_notice` и уходит
    в query fallback только если source значение blank/missing.
  - тот же drift-class закрыт и для `client_budget_host_control_launch`
    chat-notice wrapper: он больше не держит отдельно hardcoded `kind` /
    `feedback_kind`, а предпочитает canonical producer-side `operator_notice`
    fields и уходит в старый fallback только при missing/blank source values.
  - этот же launch chat-notice wrapper теперь выровнен до полного source-first
    контракта: `command_id` и `message_text` тоже больше не считаются
    implicitly canonical из sibling payload, а предпочитают
    producer-side `operator_notice` и уходят в старый computed fallback только
    если source поля blank/missing.
  - тот же launch helper теперь добит и по thread identity: `thread_id` /
    `thread_id_hint` больше не доверяют аргументу wrapper-а сильнее, чем уже
    materialized `operator_notice.thread_id`, и уходят в arg fallback только
    если source thread-id blank/missing.
  - sibling source-first drift теперь закрыт и для non-thread
    `client_budget_host_control_feedback` top-level `chat_notice`: observe API
    больше не держит отдельную локально собранную identity/message ветку поверх
    уже materialized `operator_notice`, а предпочитает source
    `kind` / `message_text` / `feedback_kind` и уходит в старый computed
    fallback только если source поля пустые или отсутствуют.
  - этот же feedback chat-notice helper теперь добит до полного source-first
    identity contour: `command_id` тоже больше не берётся только из sibling
    payload, а предпочитает producer-side `operator_notice.command_id` и
    уходит в старый fallback только если source значение blank/missing.
  - feedback chat-notice contour теперь симметрично выровнен и по thread
    identity: producer-side `operator_notice` materialize-ит `thread_id`, а
    helper предпочитает `operator_notice.thread_id` и падает назад на arg
    fallback только если source thread-id blank/missing.
  - этот же source-first law теперь симметрично закрыт и по `reply_prefix`:
    `client_budget_target_update`, `client_budget_host_control_launch` и
    non-thread `client_budget_host_control_feedback` больше не тянут
    `chat_notice.reply_prefix` только из sibling guard/payload, а materialize-ят
    `operator_notice.reply_prefix` на producer-side и предпочитают source
    notice в observe wrappers, уходя в старый guard fallback только при
    blank/missing source.
  - sibling compact-chat contour теперь тоже выровнен по thread identity:
    `continuity_compact_chat` materialize-ит `operator_notice.thread_id`, а
    observe wrapper для `delivery_surface_notice/chat_notice` предпочитает
    source thread-id и уходит в query fallback только если source значение
    blank/missing, то есть compact-chat notice больше не держит thread identity
    только как outer wrapper projection.
  - последний прямой top-level notice builder в `observe_page_api` тоже больше
    не живёт отдельно от source-of-truth: `agent_display_name_update` теперь
    materialize-ит canonical `operator_notice` внутри payload и уже от него
    строит `chat_notice`, вместо локального inline-JSON без canonical notice
    surface.
  - non-observe retrieval contour тоже выровнен на тот же truth-law:
    same-thread cache-reuse payload для context pack теперь materialize-ит
    `working_state_write_status` прямо в reuse builder, а не полагается молча
    на более поздний token-budget side-effect; из-за этого degraded marker не
  - target-update observe wrapper теперь тоже выровнен до одного
    source-first builder: top-level `chat_notice` больше не собирается
    полуручным inline JSON прямо в handler, а materialize-ится через общий
    helper `client_budget_target_chat_notice_payload`, который предпочитает
    canonical producer-side `operator_notice` fields и уходит в старые
  - compact-chat observe wrapper теперь тоже выровнен до одного helper-level
    source-of-truth contour: `delivery_surface_notice` / `chat_notice` больше
    не живут как локальная inline-ветка в handler, а materialize-ятся через
    `compact_chat_delivery_surface_notice_payload`, так что canonical source
    fields и existing kind/thread fallbacks закреплены в одном месте, а не в
    ручной дублирующей сборке.
  - после этого full observe-sweep по `src/observe` больше не находит живых
    top-level `chat_notice` / `delivery_surface_notice` inline builders этого
    drift-класса: остаются только helper-based сборки
    (`client_budget_target_chat_notice_payload`,
    `client_budget_host_control_launch_chat_notice`,
    `client_budget_host_control_feedback_chat_notice`,
    `compact_chat_delivery_surface_notice_payload`) и один прямой
    pass-through `chat_notice = operator_notice` для
    `agent_display_name_update`.
  - следующий non-observe consumer contour тоже теперь прикрыт единым
    contract-proof: `amai_context_pack` в `mcp.rs` больше не полагается только
    на разрозненные helper-тесты для warning/payload preservation, а собирает
    итоговый tool surface через `context_pack_tool_result_payload`; отдельный
    regression теперь доказывает, что degraded
    `working_state_write_status` одновременно сохраняется в
    `structuredContent.context_pack` и доезжает до compact `content[0].text`
    summary-warning.
  - соседний `continuity_startup` MCP surface тоже теперь прикрыт end-to-end
    contract-test: помимо alias `delivery_surface_restore == chat_start_restore`
    теперь отдельно доказано, что compact `content[0].text`,
    `continuity_startup_summary` и delivery-surface alias остаются согласованы
    между собой, то есть startup tool-result больше не опирается только на
    разрозненные structural assertions.
  - следующий summary-first MCP слой тоже добит пакетом, а не по одному tool:
    `amai_token_report`, `amai_memory_matrix` и `amai_observe_snapshot` теперь
    имеют end-to-end `tool_result` contract-tests, которые связывают compact
    `content[0].text` с соответствующими `*_summary` блоками в
    `structuredContent`, вместо прежнего summary-only покрытия.
  - low-priority informational MCP surfaces тоже теперь выровнены тем же
    contract pattern: `amai_list_projects`, `amai_list_namespaces`,
    `amai_stack_preflight` и `amai_benchmark_coverage` получили end-to-end
    `tool_result` assertions, так что compact text и summary blocks больше не
    живут как независимые непроверенные projections.
    message/kind/thread/reply-prefix fallbacks только при blank/missing source
    значениях.
    теряется на CLI/model-visible cache-reuse path, если caller читает payload
    до attach-side-effects.
  - отдельно закрыт и no-notice dashboard fallback для
    `host_current_thread_control_feedback`: если direct payload уже несёт
    enriched `message_text`, dashboard больше не переаппендит тот же warning в
    fallback toast path; warning append остаётся только для truly missing source
    message.
  - это прикрыто unit-тестами
    `append_working_state_warning_to_message_preserves_base_without_warning`,
    `append_working_state_warning_to_message_appends_degraded_warning` и
    `append_working_state_warning_to_message_ignores_whitespace_warning`,
    plus observe-page guards
    `client_budget_target_notice_message_prefers_source_level_notice_without_reappending`
    , `client_budget_target_notice_message_falls_back_to_default_with_warning`
    и `client_budget_target_notice_message_falls_back_when_source_notice_is_null`,
    plus dashboard structural guard
    `dashboard_html_uses_source_notice_fallback_helper_for_enriched_write_status_surfaces`,
    plus host-control launch notice guard
    `client_budget_host_control_launch_chat_notice_preserves_working_state_write_status`
    , `client_budget_host_control_launch_chat_notice_preserves_missing_write_status_as_null`
    и `client_budget_host_control_launch_chat_notice_falls_back_when_source_fields_blank`,
    plus host-control feedback identity guards
    `client_budget_host_control_feedback_chat_notice_prefers_source_notice_fields`
    и
    `client_budget_host_control_feedback_chat_notice_falls_back_when_source_notice_blank`,
    plus runtime compaction guards
    `compact_working_state_restore_for_budget_preserves_host_feedback_write_status`
    и
    `compact_working_state_restore_for_budget_preserves_missing_host_feedback_write_status_as_null`,
    plus companion broad contract proof
    `compact_working_state_restore_for_budget_keeps_only_budget_critical_fields`,
    plus top-level preview proof
    `compact_budget_snapshot_preview_payload_trims_hourly_burn_and_alignment_to_essential_fields`,
    plus override-precedence guards
    `compact_latest_repo_working_state_restore_from_optional_payload_preserves_override_payload`
    и
    `compact_latest_repo_working_state_restore_from_optional_payload_preserves_missing_payload_as_empty_restore`,
    plus compact-chat summary guard
    `compact_chat_api_summary_drops_heavy_runtime_fields`,
    plus compact-chat delivery-surface notice-kind guards
    `compact_chat_delivery_surface_notice_kind_prefers_source_notice_kind`
    и
    `compact_chat_delivery_surface_notice_kind_falls_back_to_host_launch_status`,
    plus target-update notice-kind guards
    `client_budget_target_notice_kind_prefers_source_notice_kind`
    и
    `client_budget_target_notice_kind_falls_back_when_source_kind_missing`,
    plus host-control launch notice identity guards through
    `client_budget_host_control_launch_chat_notice_preserves_working_state_write_status`
    и
    `client_budget_host_control_launch_chat_notice_preserves_missing_write_status_as_null`,
    plus host-control feedback notice identity guards
    `client_budget_host_control_feedback_chat_notice_prefers_source_notice_fields`
    и
    `client_budget_host_control_feedback_chat_notice_falls_back_when_source_notice_blank`.
  - non-dashboard MCP consumer gap вокруг `retrieval_context_pack` тоже теперь
    закрыт: `amai_context_pack` plain-text summary больше не теряет degraded
    `working_state_write_status.warning`, если structured `context_pack` уже
    знает, что primary write persisted, а `refresh_restore_snapshot` degraded.
  - теперь compact MCP summary сохраняет свой короткий `ctx d=...` stats-prefix,
    но при degraded write-state честно доклеивает warning text, так что
    summary-first consumers не зависят от structured JSON, чтобы увидеть
    degraded-after-primary-write состояние.
  - это прикрыто unit-тестами
    `context_pack_tool_summary_appends_working_state_warning_when_present`,
    `append_working_state_warning_to_compact_summary_keeps_compact_summary_without_warning`
    и companion guard `context_pack_tool_payload_stays_compact_for_model_visible_output`.
  - соседний proof-harness для schema-sensitive `restore_pack` migration tests
    теперь serializes shared DDL contour через advisory lock на весь critical
    section, а не только на отдельные `ALTER TABLE` шаги, поэтому companion tests
    не ловят ложный `deadlock` при параллельных cargo-process проверках.
    который вставляет dirty orphan, запускает custom migration string с
    intentional SQL failure после `DELETE`, и доказывает, что orphan row
    остаётся на месте после rollback.
  - source-of-truth drift внутри самого bootstrap SQL для `restore_packs` тоже
    теперь закрыт: оба `CREATE TABLE IF NOT EXISTS ami.restore_packs` блока
    выровнены на один и тот же закон `ON DELETE RESTRICT` + conditional
    `workspace_restore_pack` source identity `CHECK`, так что свежий bootstrap
    и upgrade-path больше не расходятся по этой политике.
  - это прикрыто текстовым regression test
    `bootstrap_sql_restore_packs_create_blocks_keep_source_identity_law_aligned`,
    который проверяет обе create-секции `sql/000_bootstrap.sql` и fail-closed
    краснеет, если хотя бы один блок снова уйдёт в `ON DELETE SET NULL` или
    потеряет conditional `source_snapshot` invariant.
  - schema-layer truth boundary для same-source canonical row теперь materialized
    уже не только в Rust helper-логике, но и в PostgreSQL:
    `sql/000_bootstrap.sql` сначала канонически дедуплицирует старые
    `restore_packs` по `(project_id, namespace_id, pack_kind, source_snapshot_id)`,
    оставляя самую новую строку, а затем поднимает partial unique index
    `idx_ami_restore_packs_same_source_snapshot`.
  - это прикрыто raw runtime test
    `restore_packs_schema_rejects_raw_duplicate_same_source_snapshot`, который
    обходит helper `create_restore_pack` и доказывает, что вторая строка с тем же
    `source_snapshot_id` режется уже самим PostgreSQL `UNIQUE`-барьером.
  - bootstrap/migration-safety для этого schema-layer тоже теперь доказан:
    тот же dedupe+index блок можно безопасно прогонять на грязной базе с уже
    существующим same-source дублем и потом повторять второй раз без побочных
    эффектов.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_prefers_newer_source_time_over_later_insert_and_is_idempotent`,
    который доказывает две вещи сразу:
    - dirty same-source duplicate схлопывается по канонической политике
      `captured_at_epoch_ms DESC NULLS LAST, created_at DESC, restore_pack_id DESC`;
    - повторный прогон bootstrap-блока остаётся идемпотентным и не ломает уже
      очищенное состояние.
  - это же теперь согласовано и с project law из `docs/OPERATIONS.md`: для
    historical delayed imports canonical row определяется сначала по semantic
    source-time (`captured_at_epoch_ms`), а не по позднему времени записи в БД.
  - это прикрыто ещё одним runtime test
    `restore_packs_bootstrap_dedupe_prefers_higher_source_time_even_if_inserted_earlier`,
    который доказывает, что более свежий по source-time пакет выживает даже если
    его строка была записана раньше, чем более поздний DB insert.
  - второй ключ этого же canonical `ORDER BY` тоже теперь доказан явно: если
    `captured_at_epoch_ms` совпал, bootstrap переключается на `created_at DESC`,
    и более новый DB row выигрывает tie честно и предсказуемо.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_uses_created_at_when_source_time_ties`.
  - соседний schema-upgrade hazard с equal `created_at` тоже теперь закрыт:
    если у исторически грязных same-source дублей одинаковый `created_at`,
    canonical row больше не выбирается по случайному `UUID`, а предпочитает
    более высокий `captured_at_epoch_ms`.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_prefers_higher_captured_at_when_created_at_ties`.
  - null/битый `captured_at_epoch_ms` в исторических дублях тоже уже закрыт тем
    же migration-контуром: поскольку в bootstrap такой случай деградирует в `NULL`,
    dedupe при равном `created_at` теперь предпочитает строку с реальным
    `captured_at_epoch_ms`, а `NULL` уходит вниз через `NULLS LAST`.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_prefers_non_null_captured_at_when_created_at_ties`.
  - этот `NULLS LAST` path теперь отдельно доказан и для более жёсткого случая,
    когда строка без source-time приходит позже по `created_at`: даже тогда
    поздний DB insert не может вытеснить строку с реальным `captured_at_epoch_ms`,
    потому что bootstrap остаётся source-time-first.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_prefers_non_null_source_time_over_later_created_at_with_null_source_time`.
  - самый нижний historical fallback тоже теперь доказан явно: если у same-source
    дублей одновременно нет semantic source-time с обеих сторон и `created_at`
    тоже совпал, bootstrap не оставляет выбор случайности, а детерминированно
    доходит до последнего tie-break `restore_pack_id DESC`.
  - это прикрыто runtime test
    `restore_packs_bootstrap_dedupe_uses_restore_pack_id_as_last_tiebreak_when_source_time_missing`.
  - соседний ambiguity-layer самого `create_restore_pack` тоже больше не немой:
    если `workspace_restore_pack` уже успел записаться, а caller получил
    `outcome_unknown_after_write`, `materialize_restore_pack` теперь
    восстанавливает canonical row по
    `project/namespace/pack_kind/source_snapshot_id` и не обрывает refresh path
    ложной ошибкой.
  - это прикрыто тремя отдельными тестами:
    - `create_restore_pack_forced_outcome_unknown_after_write_keeps_row_materialized`;
    - `create_restore_pack_forced_before_write_failure_leaves_no_row`;
    - `force_refresh_restore_snapshot_outcome_unknown_restore_pack_create_still_completes`.
  - после hostile proof был пойман отдельный runtime defect: worktree launcher
    `scripts/amai_exec.sh` оказался повреждённым как `0-byte` файл без
    executable bit. Канонический launcher восстановлен, а
    `./scripts/proof_continuity_shell_release_fallback.sh` усилен явной
    проверкой целостности launcher-а до proof и после trap-restore, чтобы
    следующий такой drift не проходил незаметно. Дополнительно добавлен
    hostile regression `./scripts/proof_continuity_shell_launcher_integrity_guard.sh`,
    который специально портит launcher до `0-byte` и требует немедленного
    fail-closed по preflight check, а не позднего зависания внутри proof.
  - соседний budget guard fail-open закрыт: `client_budget_reply_gate.sh`
    больше не имеет права тихо вернуть `0`, если не смог получить вообще
    никакой gate payload. Теперь он fail-closed пишет
    `client budget reply gate: no gate payload available` и завершаетcя
    отдельным кодом ошибки. Добавлен regression proof
    `./scripts/proof_client_budget_reply_gate_fail_closed.sh`, который
    временно выключает cache/gate helper и принудительно проверяет blocking-mode;
    при этом старый `./scripts/proof_client_budget_reply_gate_cache_fallback.sh`
    остаётся зелёным и доказывает, что нормальный cache fallback не сломан.
    Отдельно добавлен hostile malformed-payload proof
    `./scripts/proof_client_budget_reply_gate_invalid_payload_fail_closed.sh`,
    который подсовывает битый `client_budget_gate_cache.json` и требует
    explicit `client budget reply gate: invalid gate payload`, а не молчаливый
    проход или неявное падение глубже по jq/read path.
  - соседний `client_budget_system_markers.sh` тоже больше не имеет права
    quietly succeed без `root_cause` payload. Если ни shell helper, ни cache,
    ни direct binary fallback не смогли materialize `client_budget_root_cause`,
    script теперь fail-closed пишет
    `client budget system markers: no root cause payload available` и
    завершаетcя отдельным кодом ошибки вместо пустого “успешного” выхода.
    Добавлен hostile proof
    `./scripts/proof_client_budget_system_markers_fail_closed.sh`; при этом
    обычный `./scripts/client_budget_system_markers.sh` по-прежнему строит
    валидный marker JSON в нормальном runtime path.
  - deeper sibling drift закрыт и в самом `client_budget_gate.sh`: раньше при
    `--enforce-reply-gate` и полном отсутствии всех источников (`API`, cache,
    `client_budget_root_cause.sh`, `amai_exec.sh`, release/debug binary) script
    падал только неявно через `set -e` после пустого `gate_json`, без
    contract-level ошибки и без отдельного regression guard. Теперь он
    fail-closed пишет `client budget gate: no gate payload available` и
    завершаетcя кодом `12`. Добавлен hostile proof
    `./scripts/proof_client_budget_gate_fail_closed.sh`, который специально
    убирает все источники payload и требует именно этот explicit fail-closed.
    Отдельно добавлен hostile malformed-payload proof
    `./scripts/proof_client_budget_gate_invalid_payload_fail_closed.sh`,
    который подсовывает битый `client_budget_gate_cache.json` и требует тот же
    explicit fail-closed, а не неявное падение глубже по пайплайну.
  - ещё один соседний runtime хвост закрыт в `client_budget_root_cause.sh`:
    раньше при полном отсутствии всех источников (`API`, cache, `amai_exec.sh`,
    release/debug binary) script завершался грязным launcher-path error вроде
    `timeout: failed to run ... scripts/amai_exec.sh: No such file or directory`
    с кодом `127`, а не контрактной ошибкой. Теперь он explicit fail-closed
    пишет `client budget root cause: no root cause payload available` и
    завершаетcя кодом `12`. Добавлен hostile proof
    `./scripts/proof_client_budget_root_cause_fail_closed.sh`, а существующий
    `./scripts/proof_client_budget_root_cause_cache_fallback.sh` остаётся
    зелёным и подтверждает, что живой cache fallback не сломан.
  - соседние fallback-контуры тоже выровнены: `default_startup_next_action` и
    `rotate-chat` больше не считают отсутствие `resume_state` эквивалентом
    `clear`; resume-обязательство сохраняется только когда состояние реально
    присутствует, а missing-state больше не превращается в ложный “чистый”
    startup/rotate режим.
  - compact runtime summary тоже перестал подменять unknown-state ложным `false`:
    если `reply_execution_gate.preserves_return_obligation` не surfaced из
    action-bundle или top-level gate, compact startup runtime artifact теперь
    сохраняет `null`, а не делает вид, что обязанность возврата точно не
    сохраняется.
  - structured `execctl_resume_obligation` теперь тоже fail-closed по count:
    если `pending_return_count` в contract отсутствует, summary больше не
    подставляет ложный `0`, а сохраняет `null`, чтобы unknown-state не
    маскировался под “возвратов точно нет”.
  - top-level `chat_start_restore.thread_count` теперь тоже честный: если
    bootstrap summary не surfaced число thread-ов, restore pack больше не
    подставляет `0`, а сохраняет `null`, чтобы отсутствие данных не звучало как
    “индекс точно пуст”.
  - synthetic continuity fallback из `working_state + handoff` тоже перестал
    сочинять нули: при отсутствии continuity import он больше не пишет
    `documents_imported = 0`, `rendered_transcript_files = 0`,
    `session_memory_files = 0` и `thread_count = 0`, а сохраняет `null`, чтобы
    fallback не выдавал unknown-state за “точно пусто”.
  - `continuity_answer.chat_lookup.messages_count` теперь тоже честный: если
    chat lookup не нашёл чат, payload больше не подставляет `0`, а сохраняет
    `null`, чтобы “чат не найден” не звучало как “в найденном чате точно ноль
    сообщений”.
  - та же правка проведена и в blocked continuity-answer ветке: когда ответ
    fail-closed блокируется client-budget guard и chat lookup не выполнялся,
    `chat_lookup.messages_count` больше не surface-ится как `0`, а остаётся
    `null`, чтобы blocked-path не расходился с обычным continuity-answer JSON.
    притворяется валидным resume-contract сообщением.
  - рядом всплыл и был устранён stale-test drift: canonical fixture для
    `inspect_startup_runtime_state_reports_gate_semantics_consistent` доведена до реального
    startup/runtime payload shape (`required_task_set*`, task tree/ledger, client budget guard),
    чтобы тест проверял текущий контракт, а не устаревшую урезанную заготовку.
- bootstrap lane больше не ломает Stage 3 identity-proof на повторном schema apply:
  - `sql/000_bootstrap.sql` переведён с неидемпотентного `DROP/ADD CONSTRAINT` на guarded `pg_constraint`-aware add-path для `import_packets_derivation_kind_check`;
  - после этого `proof_execctl_resolved_task_identity.sh` снова materialize-ит зелёный verdict, а не падает раньше на schema bootstrap.

### Этап 3A. Ранний procedural seed contour

Уже materialized:
- `./scripts/proof_procedural_seed.sh`
- `./scripts/proof_procedural_shadow_review.sh`
- `./scripts/proof_restore_execution_card.sh`
- `./scripts/proof_shared_promotion_by_approval.sh`
- `./scripts/review_procedural_shadow_mode.sh`
- `./scripts/proof_observability.sh`
- evaluator/debug traces (`amai skill review --skill-card-id ...`)
- manual shadow-mode review (`./scripts/review_procedural_shadow_mode.sh amai continuity`)

Дополнительно добито:
- dedicated Stage 3A proofs больше не живут на устаревшем basis-free happy-path:
  - `proof_procedural_seed.sh` и `proof_procedural_shadow_review.sh` теперь протаскивают recorded basis (`source_event_ids / artifact_refs / evidence_span / source_kind`) через `skill_trigger_match`, `skill_trial_run` и `skill_eval`;
  - negative contour в `proof_procedural_seed.sh` теперь проверяет именно нужный fail-closed path: candidate с recorded basis materialize-ится, но `promote_verified` без evidence bundle и successful trial по-прежнему запрещён;
- ручная live-проверка Stage 3A заново подтверждена на реальном CLI/SQL path:
  - все roadmap-поля `skill_card` materialized в truth-layer и возвращаются через `amai skill review`;
  - truth tables `skill_evidence_bundles / skill_trigger_matches / skill_trial_runs / skill_evals / skill_reuse_logs` реально наполняются, а не остаются nominal schema shell;
  - `execution-card` по-прежнему скрывает `candidate/shadow/trial` из default path и surface-ит `trial` только через explicit `--allow-trial`;
  - `execution-card` теперь materialize-ит operational metadata, а не только минимальный apply shell:
    - `--context` реально фильтрует по `skill_context_constraints`;
    - payload теперь несёт `skill_trigger_conditions`, `skill_scope_type`, `skill_owner_scope`;
    - выдача ранжируется по `skill_trust_state -> skill_utility_score -> reuse/success/failure`, а не идёт в произвольном порядке из list-surface;
  - evaluator/trial/reuse paths стали stricter (fail-closed) против накрутки и ложной промоции:
    - `promote_shadow / promote_trial / promote_verified` требуют хотя бы один `skill_trigger_match` с `matched=true`;
    - `record-trial-run` больше не считает `success/failure`, если `matched=false` или (не shadow) `applied=false`;
    - `record-reuse` требует `matched=true` и `applied=true` в evidence-span для non-neutral outcome;
    - `candidate_only` запрещает менять utility, а `reject/quarantine/deprecate` запрещают увеличивать utility.
  - `promote_verified` без evidence/trial остаётся fail-closed и вручную, не только в proof.
  - dedicated `negative procedural memory` proof теперь materialize-ит и прогоняет весь verified path не только для `anti_pattern / failure_playbook / repair_sequence`, но и для `failure_pattern`, а затем проверяет их coexistence рядом с success-skill на общем execution surface:
    - `./scripts/proof_negative_procedural_memory.sh`
    - Rust non-regression: negative procedural classes реально поднимаются через `build_skill_execution_cards`, а не только существуют как schema labels.
  - `skill patching instead of clone explosion` больше не висит как design-only обещание:
    - похожий skill без explicit refinement decision теперь fail-closed отклоняется;
    - patch требует `--patch-parent-skill-card-id`, сохраняет version lineage и пишет `skill_patch_parent_id`;
    - merge пишет `skill_merge_group_id`;
    - explicit `new` допускается, но только как осознанное отклонение и материализуется в `skill_refinement_decision` внутри evidence span;
    - CLI proof: `./scripts/proof_skill_refinement_contour.sh`
  - `versioned skill history` больше не ограничивается одним полем `skill_version`:
    - `skill create-candidate` принимает `--changed-by` и `--change-reason`, и эти данные materialize-ятся в durable evidence span;
    - `skill review` теперь surface-ит ordered `history` по lineage/merge-group с actor, reason, refinement action и patch parent;
    - отдельный proof contour: `./scripts/proof_skill_version_history.sh`;
    - proof contour теперь проверяет не только `v1 -> v2`, но и merge-group lineage, а также то, что history не теряется после `add-evidence -> record-trigger-match -> promote_shadow -> promote_trial -> record-reuse`;
    - ручная CLI-сверка подтвердила и patch-history, и merge-history в живом review JSON, а не только в unit-test.
  - `restore as execution card` теперь materialized в реальном restore path, а не только в отдельном `skill execution-card` surface:
    - `working_state_restore` теперь поднимает `skill_execution_card`, `skill_execution_card_summary` и `skill_execution_card_binding`;
    - `chat_start_restore` теперь surface-ит ту же компактную карточку и добавляет строку `Карточка: ...` в prompt;
    - selection идёт fail-closed: без runtime/model/tool binding или без релевантного trial-card restore не подсовывает procedural note вместо execution card;
    - отдельный proof contour: `./scripts/proof_restore_execution_card.sh`;
    - ручная CLI-сверка подтверждает, что в prompt поднимается именно компактная карточка для текущего шага, а не длинная procedural простыня.
  - `shared promotion by approval` теперь materialized как отдельный truth-layer gate, а не implicit side-effect от `promote_verified`:
    - `project_shared` skill после `promote_verified` остаётся `skill_shared_promotion_state = pending_approval`;
    - `build_skill_execution_cards` fail-closed скрывает `project_shared` verified skill, пока evaluator/trust contour не запишет `approve_shared_promotion`;
    - после explicit approval карточка начинает surface-иться в shared execution path и сохраняет `skill_shared_approved_by / skill_shared_approval_reason / skill_shared_approved_at`;
    - отдельный proof contour: `./scripts/proof_shared_promotion_by_approval.sh`;
    - ручная live-сверка на отдельном namespace подтвердила exact переход `pending_approval + execution_card_hits=0 -> approved + execution_card_hits=1`.

Закрывающий non-regression bundle:
- `./scripts/proof_working_state_decision_trace.sh`
- `./scripts/proof_execctl_resolved_task_identity.sh`

Важно:
- dedicated proof и review surface теперь materialized как first-class contour, а checkbox закрывается только после shadow-mode review и честной stage-сверки;
- seed contour не равен full procedural memory из Этапа 8.

### Этап 4. Workspace restore pack

Текущий честный статус:
- этап закрыт;
- startup/restore/observed continuity bundle прогнан полностью;
- raw continuity verifier для `art/continuity` зелёный;
- следующий implementation focus теперь переносится на `Этап 5`.

Что уже прошло:
- `./scripts/proof_art_continuity_startup.sh`;
- `./scripts/proof_art_continuity_restore.sh`;
- `./scripts/proof_workspace_restore_pack_acceptance.sh`;
- `./scripts/proof_workspace_restore_pack_hardening.sh`;
- `./scripts/proof_token_continuity_restore_observed.sh`;
- `cargo run --quiet -- verify continuity --project art --namespace continuity`.

Что подтвердило закрытие этапа:
- startup и restore больше не расходятся по proof-критичным полям;
- новая рабочая поверхность поднимает не только headline, а полезный рабочий пакет;
- observed continuity contour честно видит restore без token-truth drift.
- отдельный acceptance-proof принудительно проверяет:
  - `blocked/waiting` как непустой bucket;
  - `relevant_procedures` как `compact execution card`, а не raw procedural archive;
  - ручной startup/restore surface на isolated namespace, а не только live `art/continuity`.
- отдельный hardening-proof принудительно проверяет:
  - stale replay suppression для handoff/import selection;
  - reject на missing/mismatched `source_snapshot_id`;
  - reject на poisoned evidence span;
  - fail-closed поведение builder-а на malformed restore surface и raw procedural note без execution card.

### Этап 5. Semantic + temporal memory strengthening

Текущий честный статус:
- этап закрыт;
- closing proof bundle прогнан полностью и зелёный:
  - `./scripts/proof_semantic_temporal_memory.sh`
  - `./scripts/proof_semantic_temporal_manual_acceptance.sh`
  - `./scripts/proof_accuracy.sh`
  - `./scripts/proof_text_compare.sh`
  - `./scripts/proof_text_compare_real_projects.sh`
  - `cargo run --release -- verify accuracy --project project_alpha --related-project project_beta --namespace review`
- observability truth sync для surfaced accuracy/isolation contour перепроверен на том же persisted snapshot source, который читает dashboard benchmark card `Точность и изоляция`: `ami.observability_snapshots.snapshot_kind = retrieval_accuracy -> latest_retrieval_accuracy.accuracy_verification`.
- temporal factual recall и temporal truth теперь доказаны не только unit/fixture path, но и live manual acceptance contour:
  - `context pack --at-epoch-ms ...` больше не режет исторически валидные `superseded/retracted` memory objects только из-за их current-state;
  - cache isolation для temporal queries починен: `at_epoch_ms` теперь входит в local/fast context-pack cache key и старый временной срез не переиспользуется как replay для нового.
  - retrieval `decision_trace` теперь materialize-ит explicit `rerank_legality_relevance.temporal_legality`, чтобы было видно, что historical-but-valid candidates остались допустимыми именно на запрошенном timestamp, а не всплыли как обычный latest-state hit.
  - temporal legality explainability усилен до prefilter/exclusion surface: в живом retrieval trace теперь видны `prefilter_memory_cards` и `excluded_*_by_temporal_window`, так что руками можно отличить surviving historical hit от кандидата, который совпал по тексту, но был вырезан time-slice filter.
  - temporal legality explainability усилен ещё на один уровень: durable retrieval trace теперь показывает и `excluded_memory_card_candidates`, так что в ручной проверке виден не только факт exclusion, но и конкретный title/id кандидата, вырезанного как `outside_requested_time_slice`.
  - semantic `knowledge update` для `memory_card` больше не оставляет старый current fact жить рядом с новым только потому, что поменялся `fact_object`: same `fact_subject + fact_predicate` теперь честно ведут к supersession old fact, relation edge `supersedes` и recorded truth-state transition.
  - manual retrieval boundary на generic factual NL query устранён: memory-card retrieval теперь матчится не только по `title/summary/body`, но и по `fact_subject / fact_predicate / fact_object`, а query-side normalizer вычищает шумовые question/stop words; из-за этого `context pack --at-epoch-ms ...` больше не требует искусственный lexical anchor вроде `server region`, чтобы поднять semantic fact по вопросу вида `What is the current region of infra.server.region?`.
  - negative guard для этого factual NL path тоже materialized: future-only wording вроде `When did ... move to us-east?` больше не протекает назад в pre-update time-slice, а stage proof bundle теперь явно держит и generic-NL factual retrieval, и stale-cache bypass для `verify_context_pack`.
  - `update_memory_card_truth_state(..., truth_state = retracted|superseded, ...)` теперь автоматически закрывает `valid_to_epoch_ms`, если temporal window ещё не был закрыт; за счёт этого retract path больше не оставляет ложный “бесконечно валидный” historical window и не всплывает ни в latest retrieval, ни в future slice после момента retract.
  - latest factual retrieval теперь rank-ит truth-quality выше голой свежести: `current + verified + active` card получает приоритет перед `conflicted/disputed` кандидатом, даже если conflicted claim новее или текстово “богаче”; это закрыто exact regression на mixed-state search result.
  - exact-time semantic slice теперь закрыт не только explainability trace-ом, но и live Rust proof на path `knowledge update -> supersession -> transition -> historical retrieval`.
  - verify contour больше не переиспользует stale fast-cache для `verify_context_pack`, если related-project visibility изменилась после relation/access-policy update: Stage-5 compare/proof path теперь читает честный live scope, а не старый local-only replay.
  - real-project text-compare proof теперь materialize-ит explicit `cross_project_linked` read-policy для source project, так что related-project retrieval проверяется на реальном access-policy contour, а не на случайном baseline state.

Использовать:
- `./scripts/proof_semantic_temporal_memory.sh`
- `./scripts/proof_semantic_temporal_manual_acceptance.sh`
- `./scripts/proof_accuracy.sh`
- `./scripts/proof_text_compare.sh`
- `./scripts/proof_text_compare_real_projects.sh`
- `cargo run --release -- verify accuracy ...`

### Этап 6. Multi-agent shared/private memory

Сейчас дополнительно зацементировано:
- обычный `memory_item` write-path больше не принимает cross-project basis без controlled `import_packet`;
- `memory_item` truth write теперь fail-closed, если `import_packet` указывает не в тот target contour;
- borrowed cross-project `memory_item` теперь materialize-ится с `visibility_scope = imported`, а не тихо наследует `target project` scope; из-за этого controlled transfer больше не ломается на DB trigger и не маскирует borrowed state под local truth;
- обычные `memory_item / memory_card / task_node / skill_card` write-path не могут materialize-иться внутрь `quarantine` contour; для этого нужен dedicated quarantine lane;
- для Stage 6 появился отдельный hardening proof на quarantine/shared-transfer bypass path.
- `shared_asset` contour теперь explicit-proof-ом держит `org_global`: asset обязан идти через transfer policy, same-workspace binding проходит с stage2 provenance, а cross-workspace duplicate `asset_code` больше не может тихо увести bind в чужой workspace, потому что lookup теперь workspace-scoped по target project contour.
- tester-style live acceptance теперь materialized отдельным harness: он вручную проверяет `agent_private` vs `project_shared`, controlled `cross_project_linked` transfer, `visible_projects` isolation, `org_global` same-workspace binding и duplicate-code split across workspaces.

Использовать:
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_hostile.sh`
- `./scripts/proof_load.sh`
- `./scripts/proof_shared_private_memory_hardening.sh`
- `./scripts/proof_shared_private_memory_manual_acceptance.sh`

### Этап 7. Compare + benchmark plane

Использовать:
- `./scripts/proof_token_benchmark.sh`
- `./scripts/proof_token_benchmark_suite.sh`
- `./scripts/proof_token_live_turn_savings_matrix.sh`
- `./scripts/proof_token_art_live_turn_savings.sh`
- `./scripts/proof_benchmark_matrix.sh`
- `./scripts/proof_procedural_benchmark.sh`

### Этап 8. Procedural memory

Сейчас честный статус такой:
- dedicated procedural benchmark/proof уже materialized в compare-plane:
  - `./scripts/proof_procedural_benchmark.sh`;
- compare-plane для procedural benchmark теперь materialize-ит обе benchmark-линии:
  - `with_amai`;
  - `without_amai_but_measuring` через explicit procedural bypass-run;
  - snapshot и dashboard больше не держат procedural compare в ложном `pending`, а показывают dual-line state и honest run-state;
- richer scored reporting и persisted benchmark history для procedural metrics теперь materialized:
  - `observe snapshot` поднимает `procedural_benchmark_history`;
  - dashboard-card несёт persisted history counts и separate time-series для `with Amai` и `without Amai`.

Значит:
- Stage 8 internal checkbox можно считать закрытым только как historical/internal stage verdict;
- fresh claim `full procedural memory сейчас green` нельзя повторять без rerun отдельного procedural benchmark/proof и companion evaluator/trust traces.

Fresh proof-refresh bundle:
- `./scripts/proof_procedural_seed.sh`
- `./scripts/proof_procedural_benchmark.sh`
- evaluator/trust verification через `skill review`, shared approval gate и execution-card surface;
- shadow -> trial -> verified trace review;
- dashboard/raw parity для procedural benchmark history.

Если хотя бы один пункт этого bundle падает:
- Stage 8 checkbox снимается;
- claim `full procedural memory already closed` заменяется на `partial / proof-refresh-required`;
- профильный gap описывается здесь и в `IMPLEMENTATION_GATES.md`.

### Этап 9. Forgetting, consolidation, pruning

Использовать:
- `./scripts/proof_forgetting_consolidation.sh`
- `./scripts/proof_observability.sh`

Status-truth rule:
- Stage 9 internal checkbox остаётся valid только при свежем подтверждении forgetting proof и observability companion;
- если proof не прогнан в текущей рабочей линии, свежий claim должен звучать как `proof-refresh-required`, а не как новый green verdict.

Ручная сверка реальности:
- `memory explain-forgetting` обязан возвращать action/reason/retention_class/decay_policy;
- named jobs обязаны быть surfaced через `memory run-job --job-kind de_duplication_job|summarization_job|compaction_job|pruning_job|cold_archive_job|revalidation_job`;
- `summarization_job` пока обязан быть честным explicit no-op, а не молчаливым отсутствием runtime surface;
- governance/dashboard surface обязан показывать forgetting breakdown по pruning/archive/revalidation/dedup, а не только общий audit-count;
- stale `truth_state=current` item обязан уходить в `pending_review`, а не оставаться `active/current`;
- `raw_capture / operator_write / verified_write_back / durable / legal_hold / retain_forever` не имеют права auto-prune/archive.

### Этап 10. Governance, safety, evaluator loop

Использовать:
- `./scripts/proof_hostile.sh`
- `./scripts/proof_memory_task_matrix.sh`
- `./scripts/proof_mcp_task_matrix.sh`
- `./scripts/proof_observability.sh`

Status-truth rule:
- Stage 10 internal checkbox не заменяет свежую safety/governance/evaluator проверку;
- hostile, memory matrix, MCP matrix и observability proof должны быть прогнаны вместе перед любым claim `governance/safety/evaluator loop сейчас green`;
- privacy/safety specific traces, quarantine and override paths считаются обязательными ручными audit lanes, если изменение касается shared/import/procedural/governance contour.

## Как понять, где этап начинается и где заканчивается

Простое правило:
- строка в чеклисте ведёт в точный раздел roadmap, где этот этап описан подробно;
- этап начинается не в момент, когда о нём просто поговорили, а когда он объявлен текущим focus в этом статус-документе и по нему реально пошли изменения;
- этап заканчивается не в момент, когда код написан, а только когда его stage gate реально закрыт.

Правило обновления:
- этап закрыт только тогда, когда у него есть stage gate;
- stage gate считается закрытым только после полного цикла:
  - tests;
  - manual check;
  - debug/fix;
  - retest;
- и после benchmark/proof-проверки, что не просели:
  - скорость;
  - точность;
  - качество;
  - правдивость;
- если что-то просело:
  - checkbox не ставится;
  - сначала нужен root-cause;
  - потом восстановление baseline;
  - потом повторная проверка;
- кроме этого должны быть выполнены общие правила из roadmap:
  - [stage gate](AMAI_GLOBAL_MEMORY_ROADMAP.md#1-у-каждого-этапа-должен-быть-stage-gate);
  - [migration и kill-switch plan](AMAI_GLOBAL_MEMORY_ROADMAP.md#2-у-каждого-этапа-должен-быть-migration-и-kill-switch-план);
- если этот файл обновлялся как часть значимого этапа, status snapshot нельзя считать честно обновлённым без passing:
  - `./scripts/implementation_status_sync_guard.sh --json`;
- если этап значимый, checkbox нельзя ставить без passing:
  - `./scripts/maintainability_stage_close_guard.sh --json`;
- после этого здесь меняется checkbox;
- если этап только начат, но не закрыт, checkbox не ставится.

### Что ещё не materialized в коде

Пока ещё не materialized полностью:
- полностью бесшовный host-side переход длинной рабочей линии в новую рабочую поверхность во всех клиентах и средах.
- при этом prompt-side часть этого gap уже усилена:
  - `chat_start_restore` теперь поднимает не только headline/step;
  - новая рабочая поверхность уже видит `pending_return`, `ExecCtl return contract` и `required_task_set` явно и компактно.
  - canonical compact-chat path теперь уже сам запрашивает clean chat surface, если host launch bridge доступен.
  - startup/onboarding front-door снова materialized fail-closed:
    - `.amai/onboarding/project-chat-startup-contract.json` снова materialized на canonical пути;
    - compact-chat contract уже имеет отдельные host-launch states, а runtime/API теперь различают `available_not_requested`, `bridge_unavailable`, `disabled_by_policy`, `requested` и `launch_failed`; `requested` / `launch_failed` доказаны только как opt-in shell command-contract и не считаются seamless client UX.
    - clean-chat launch support больше не generic: VS Code имеет отдельный `vscode_code_chat_cli` command-contract, non-VSCode clients surface-ят `manual_only` / `client_has_no_automatic_clean_chat_bridge`.
    - локальный one-script install/onboarding теперь ещё и materialize-ит managed `systemd --user` unit `amai-stack.service`; он закрывает восстановление stack при старте user manager, но не является доказанным headless reboot guarantee без login/linger/system-service lane.
  - operator/API surface тоже подтянут:
    - compact-chat notice больше не притворяется generic success, если automatic launch отключён политикой;
    - unavailable/not-requested/policy-disabled states теперь emitted через fail-closed runtime/API branches; failed/requested states теперь покрыты opt-in command-contract tests и всё равно не должны документироваться как seamless UX.
  - non-VSCode fallback стал конкретнее:
    - compact-chat теперь знает текущий client surface;
    - manual fallback note может показать не только `prompt_text`, но и конкретный startup/manual path клиента (`AGENTS.md`, `.cursor/rules/...`, `tmp/onboarding/...` и т.д.).
    - для fresh-chat front-door materialized и client-specific reconnect assist:
      - `./scripts/reconnect_local.sh --client ...`
      - `./scripts/amai_exec.sh bootstrap reconnect --client ... --yes`
    - dashboard KPI selector теперь тоже surface-ит этот assist прямо в compact-chat tooltip:
      - какой именно client/fresh-chat surface выбран;
      - где лежит startup surface;
      - какими reconnect/open-new-chat командами поднимать clean chat fallback вручную.

Host-side clean-chat / client startup consensus records:
- Fresh proof 2026-04-26: `./scripts/proof_mcp_orphan_cleanup.sh`, `./scripts/proof_client_reconnect.sh` and `./scripts/proof_client_clean_chat_launch.sh` passed for robust orphan MCP cleanup, reconnect assist, compact Hermes startup/profile surface, repo file restore after proof mutation, VS Code clean-chat launch command-contract, non-VSCode manual-only launch boundary, and API notice-kind preservation for fail-closed host states; this refresh does not upgrade the unresolved seamless clean-chat claim.
- `AMAI-AUDIT-CLIENT-001`
  - `claim_owner`: docs previously implied compact-chat already emits `bridge_unavailable` and `available_not_requested`;
  - `implementation_verifier`: `src/continuity/continuity_compact_chat_helpers.rs::maybe_launch_compact_chat_host` and `build_compact_chat_clean_launch_surface_with_vscode_binary`;
  - `proof_owner`: targeted `cargo test --quiet maybe_launch_compact_chat_host`, `cargo test --quiet compact_chat_clean_launch_surface`, `cargo test --quiet compact_chat_notice_kind_preserves_fail_closed_host_states`, and `./scripts/proof_client_clean_chat_launch.sh` now prove no-request -> `available_not_requested`, missing command -> `bridge_unavailable`, default policy gate -> `disabled_by_policy`, opt-in shell command success -> `requested`, opt-in shell command failure -> `launch_failed`, API notice-kind preservation for those fail-closed states, the public wrapper honoring `AMAI_COMPACT_CHAT_AUTO_LAUNCH=1`, VS Code `code chat` command-contract, missing VS Code CLI, missing prompt artifact, and non-VSCode `manual_only` gap;
  - `consensus_verdict`: `host_state_machine_command_contract`;
  - `required_doc_action`: keep emitted-state wording tied to proof-backed host command-contract branches and keep live client UX claims out of this verdict;
  - `required_implementation_action`: add live client-open/session evidence before calling the transition seamless.
- `AMAI-AUDIT-CLIENT-002`
  - `claim_owner`: Hermes startup/onboarding claim is stronger than generic MCP config writing because it also creates sticky project profile;
  - `implementation_verifier`: `config/client_targets.toml`, `src/onboarding.rs::ensure_hermes_project_profile`, `src/onboarding.rs::remove_hermes_project_profile`;
  - `proof_owner`: `proof_client_reconnect.sh` and `proof_remote_onboarding.sh` validate compact `.hermes.md`, config install/remove and reconnect helpers; Rust tests validate sticky active-profile install/remove;
  - `consensus_verdict`: `verified_working_for_profile_install_contract`;
  - `required_doc_action`: keep Hermes profile/install claim but do not equate it with full live Hermes chat behavior;
  - `required_implementation_action`: add live Hermes CLI/session proof only before claiming a fully agentic Hermes runtime behavior.
- `AMAI-AUDIT-CLIENT-003`
  - `claim_owner`: client reconnect assist is materialized, but it is not the same as seamless clean-chat migration across all hosts;
  - `implementation_verifier`: `scripts/reconnect_local.sh`, `scripts/cleanup_mcp_orphans.sh`, `scripts/proof_client_reconnect.sh`, startup contract reconnect helper fields;
  - `proof_owner`: reconnect proof kills orphan MCP process whose argv0 contains `amai mcp serve`, including spaced `/proc/<pid>/stat` command names, and verifies install/remove lifecycle for VSCode, Cursor, Codex, Claude Code, Hermes and OpenClaw;
  - `consensus_verdict`: `verified_working_for_reconnect_assist`;
  - `required_doc_action`: keep reconnect helper claim separated from host-side clean-chat auto-launch claim;
  - `required_implementation_action`: add per-client clean-chat launch proofs before calling the transition seamless.
- `AMAI-AUDIT-CLIENT-004`
  - `claim_owner`: remote onboarding proof must validate remote SSH config and repo-sync integration without depending on live DNS or a real `example-host`;
  - `implementation_verifier`: `scripts/onboard_remote_client.sh`, `scripts/sync_remote_repo.sh`, `scripts/proof_remote_onboarding.sh`, `scripts/proof_remote_repo_sync_payload.sh`;
  - `proof_owner`: `proof_remote_onboarding.sh` now uses a local fake-ssh harness for cleanup, tar extraction and post-sync file checks; `proof_remote_repo_sync_payload.sh` validates the synced payload excludes runtime-heavy paths;
  - `consensus_verdict`: `verified_working_for_offline_remote_onboarding_proof`;
  - `required_doc_action`: keep remote onboarding proof claims tied to deterministic fake-ssh/payload evidence, not to availability of a live remote host;
  - `required_implementation_action`: keep any future live-SSH/e2e proof as a separate optional environment-bound lane, not as the default offline proof.

Local stack autostart consensus records:
- Fresh proof 2026-04-25: `./scripts/proof_stack_autostart.sh`, `./scripts/proof_bootstrap_volume_dirs.sh` and `./scripts/proof_onboarding.sh` passed. The onboarding proof also produced `tmp/onboarding/proof-vscode.out` with `Amai stack autostart ready: amai-stack.service` and the rendered user-unit path. This refresh verifies the user-manager autostart contract only; it does not add a reboot/linger/headless proof.
- `AMAI-AUDIT-AUTOSTART-001`
  - `claim_owner`: docs previously said local one-script install/onboarding makes stack return after ordinary reboot without manual `./scripts/bootstrap_stack.sh`;
  - `implementation_verifier`: `src/onboarding.rs` calls `scripts/install_stack_autostart.sh`; the script renders `amai-stack.service` with `WantedBy=default.target`, `ExecStart=scripts/run_stack_service.sh`, `Type=oneshot`, `RemainAfterExit=yes` and `systemctl --user enable --now`;
  - `proof_owner`: `./scripts/proof_stack_autostart.sh` verifies deterministic unit rendering; `./scripts/proof_bootstrap_volume_dirs.sh` verifies the bootstrap launcher prepares required volume/config artifacts before compose startup; `./scripts/proof_onboarding.sh` checks the human install output when real user-systemd is available; all three were rerun green on 2026-04-25;
  - `consensus_verdict`: `partial`;
  - `required_doc_action`: downgrade reboot wording to `systemd --user unit is materialized and enabled for the user manager`, and state that unattended boot without login requires a separate linger/system-service proof lane;
  - `required_implementation_action`: add explicit `loginctl enable-linger` detection/opt-in or a system-level service mode, plus a proof that validates post-reboot/headless semantics before restoring a broad reboot guarantee.

### Что сейчас в работе

Сейчас активный implementation focus:
- удержание Stage 0-10 bundle в зелёном non-regression состоянии после закрытия MCP matrix / observability red-state;
- scientific reinforcement overlay поверх Stage 7/9/10:
  - [AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md](AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md);
  - это уже не просто synthesis-note, а authoritative execution-spec для advisory/proof-grade probabilistic/statistical contours;
  - `Candidate Queue 4A` в этом документе now holds the KAN/context-pack utility
    explain contract; until `shadow_approved`, any new session must treat it as
    spec-only and not as a materialized feature;
  - document authorizes production materialization только для:
    - statistical benchmark honesty;
    - lifecycle transition discipline;
    - `Markov / hazard lifecycle v1` advisory contour;
    - regression explain surface;
    - Poisson/arrival capacity forecast;
  - document explicitly не authorizes:
    - truth-authoritative Bayesian promotion;
    - destructive probabilistic auto-decision;
    - replacement of `verified truth` with projection.

### Ближайший следующий этап

Stage status после fresh proof-refresh 2026-04-24:
- Этап 0-10: ✅ закрыты;
- `Этап 10. Governance, safety, evaluator loop` снова fresh-green после fix MCP matrix / observability red-state и clean rerun полного Stage 10 bundle.

Коротко:
- scope/identity уже закрыт;
- typed envelope/provenance уже закрыт;
- graph/task layer уже закрыт;
- ранний procedural seed contour уже закрыт;
- workspace restore pack уже закрыт;
- semantic + temporal strengthening уже закрыт;
- multi-agent shared/private memory уже закрыт;
- compare + benchmark plane уже закрыт;
- full procedural memory уже закрыт;
- forgetting/consolidation/pruning уже закрыт;
- governance/safety/evaluator loop закрыт fresh-green: implementation surfaces есть, свежий proof bundle зелёный;

Параллельно с удержанием Stage 10 в clean-green следующий честный надстроечный contour такой:
- scientific reinforcement memory layer как queue-driven execution overlay;
- это не новый отдельный stage и не override текущего roadmap;
- это execution program поверх уже закрытых Stage 7 / 9 / 10 с canonical order:
  - Queue 0: preflight and baseline freeze;
  - Queue 1: statistical benchmark honesty;
  - Queue 2: lifecycle transition discipline;
  - Queue 3: `Markov / hazard lifecycle v1`;
  - Queue 4: regression explain surface;
  - Queue 5: Poisson / arrival capacity forecast.

Текущий честный статус направлений из scientific execution-spec:
- `Queue 0 baseline freeze launcher`
  - `materialized`
  - canonical `v1` launcher теперь есть: `./scripts/scientific_queue0_baseline_freeze.sh`
  - он уже пишет machine-readable manifest baseline artifact set, exact command ledger, checksums, initial/final worktree fingerprints и strict `--require-clean-worktree` preflight-fail path.
  - dedicated launcher-contract proof теперь тоже materialized: `./scripts/proof_scientific_queue0_baseline_freeze.sh`
    - он держит `--print-plan` contract, dirty-worktree fail-closed path, symlink на `latest` run и machine-readable manifest shape для `preflight_failed_dirty_worktree`.
- `confidence/calibration`
  - `partially_materialized`
  - measured approval overlay уже materialized как отдельный layer поверх compare/promotion plane, но финальный promotion всё ещё требует явного human sign-off.
- `benchmark significance + drift`
  - `in_progress`
  - materialized Queue 1 slices сейчас такие:
    - `memory_task_matrix` и `mcp_task_matrix` теперь публикуют unified fail-closed `statistics` block (`benchmark-statistics-v1`);
    - block теперь несёт `sample_size`, explicit `baseline_run_id / candidate_run_id`, measured Wilson 95% CI для success-rate, measured bootstrap percentile 95% intervals для pairwise deltas и measured JSD/KS drift methods по second-run baseline pair;
    - applicability теперь surface-ится truthfully:
      - `memory_task_matrix` materialize-ит measured `score_delta`, `median_latency_delta`, `p95_latency_delta`, `verdict_distribution_drift`, `latency_distribution_drift`, а `mean_delta` честно маркирует `not_applicable`;
      - `mcp_task_matrix` materialize-ит measured `mean_delta`, `median_latency_delta`, `p95_latency_delta`, `verdict_distribution_drift`, `latency_distribution_drift`, а `score_delta` честно маркирует `not_applicable`;
    - promotion внутри этого block больше не fail-closed из-за missing pairwise contour, но остаётся compatibility/completeness signal, а не final approval law:
      - отдельный `promotion_law` block (`benchmark-promotion-law-v1`) теперь materialized рядом со `statistics` и fail-closed различает:
        - `blocked_statistics_incomplete`
        - `blocked_benchmark_gates`
        - `candidate_ready_for_measured_approval`
      - compare-plane больше не смешивает completeness `statistics` с final promotion decision.
    - measured approval policy тоже уже materialized отдельным block:
      - `memory_task_matrix` и `mcp_task_matrix` теперь публикуют `measured_approval`;
      - когда `statistics` и `promotion_law` готовы, verdict честно становится `pending_human_review` с reason `explicit_human_signoff_required`;
      - automatic promotion всё ещё запрещён: final approval остаётся human-gated, но evidence packet теперь materialized и review-ready.
    - новый compare/drift contour уже surfaced наружу:
      - `observe snapshot` теперь публикует `latest_memory_task_matrix` и `latest_mcp_task_matrix`;
      - MCP tool summary для `amai_observe_snapshot` теперь тоже не скрывает scientific lifecycle внутри raw snapshot only:
        - `observe_snapshot_summary.latest_memory_task_matrix_summary` и `latest_mcp_task_matrix_summary` materialize-ят compact lifecycle string вида `compare=<drift_status> promotion=<promotion_law_state> approval=<measured_approval_state>`;
        - `verify mcp` теперь держит этот raw-result contract явно, а не только green SLA/unknown=0;
      - dashboard benchmark plane теперь честно materialize-ит отдельные карточки `Memory task matrix compare` и `MCP task matrix compare`, где видны `baseline/candidate` pair state, measured/not_applicable/not_measured methods, `drift_summary` и separate `promotion_law` state/reason вместо старого implicit promotion summary.
      - MCP tool summary для `amai_memory_matrix` теперь тоже больше не остаётся на old point-estimate headline only:
        - high-signal summary string materialize-ит `compare=<drift_status>`, `promotion=<promotion_law_state>` и `approval=<measured_approval_state>`;
        - structured `memory_matrix_summary` тоже surface-ит эти три поля, так что operator не обязан раскрывать весь raw payload, чтобы увидеть scientific lifecycle state.
  - companion proof status после fresh proof-refresh 2026-04-24:
    - `./scripts/proof_memory_task_matrix.sh` — green;
    - `./scripts/proof_benchmark_contamination_preflight.sh` — green;
    - `./scripts/proof_mcp_task_matrix.sh` — green after strict-heavy contamination preflight and explicit failed-run artifact handling;
    - `./scripts/proof_observability.sh` — green; dashboard `MCP task matrix compare` no longer degrades to `ещё нет данных` after the MCP matrix proof path.
    - historical fixed tails, still useful as root-cause context:
      - первый честный latency tail был локализован не в matrix threshold, а в `observe_snapshot_green -> token_budget_dashboard_report -> assistant_scope`;
      - его корень был в false cache-miss law: `assistant_scope` source cache инвалидировался по global `working_state_event / token_budget_event / assistant_generation_turn_observed` summaries и ловил чужой observability noise;
      - после переключения на targeted context-pack summaries live `assistant_scope` pre-cache path вернулся к honest hit contour примерно `~0.52s` вместо ложных `~6s` miss-path spikes;
      - следующий реальный MCP tail оказался уже в `observe_snapshot_green -> active_agent_budget -> exact_client_limits_resolution`;
      - корень там был в repeated `5s` negative exact-limit lookup: при `Err(timeout)` без stale observation resolution не materialize-ил fresh missing cache, и каждый новый process/session снова платил timeout tax;
      - после fail-closed negative shared-cache path companion MCP proof historically returned green without changing `max_p95_ms`, but this is no longer a fresh 2026-04-24 green claim.
- `Queue 2 lifecycle transition discipline`
  - `materialized_minimal_slice`
  - canonical minimal Queue 2 contour теперь materialized end-to-end:
    - `sql/000_bootstrap.sql` теперь публикует:
      - `ami.lifecycle_transition_events_v1`
      - `ami.lifecycle_transition_stats_v1`
    - forgetting CLI теперь даёт exact lifecycle stats path:
      - `cargo run -- memory transition-stats --project ... --namespace ...`
    - lifecycle report now surfaces:
      - `observed_state`
      - `next_state`
      - `derivation_kind`
      - `retention_class`
      - `decay_policy`
      - `freshness_band`
      - `utility_band`
      - `access_band`
      - `transition_count`
      - `total_dwell_ms`
      - `avg_dwell_ms`
      - `p50_dwell_ms`
      - `p90_dwell_ms`
      - `last_recorded_at`
    - bootstrap currentness law now fail-closes on lifecycle stats type drift too, instead of checking only object existence;
    - `./scripts/proof_forgetting_consolidation.sh` is green with explicit Queue 2 assertions for `pruned`, `archived`, `pending_review` and conditional `compacted` rows.
  - what is still intentionally pending:
    - observe/dashboard lifecycle projection surfaces from later Queue 2/4 contours;
    - no premature Queue 3 hazard/Markov overbuild inside this minimal slice.
- `Markov/hazard lifecycle`
  - `materialized_minimal_slice`
  - canonical Queue 3 minimal advisory contour теперь materialized поверх Queue 2 transition dataset:
    - новый CLI path:
      - `cargo run -- memory cohort-risk --project ... --namespace ...`
    - advisory report now surfaces:
      - `expected_next_state`
      - `pending_review_risk_7d`
      - `archive_risk_30d`
      - `prune_risk_30d`
      - `expected_residency_ms`
      - `cohort_reason_summary`
      - plus explicit `transition_probabilities` and `dwell_p50/p75/p90_ms`
    - cohort split remains exact and bounded to:
      - `observed_state`
      - `derivation_kind`
      - `retention_class`
      - `decay_policy`
      - `freshness_band`
      - `utility_band`
      - `access_band`
    - probability block is honest `v1`:
      - empirical transition counts from `ami.lifecycle_transition_events_v1`
      - Laplace smoothing with `alpha = 1.0`
      - no hidden states, HMM or black-box replacement
    - horizon risks stay advisory-only:
      - derived from smoothed transition probability multiplied by observed within-horizon fraction for the target next-state
      - no direct authority to run prune/archive/policy actions
    - `./scripts/proof_forgetting_consolidation.sh` now includes Queue 3 cohort-risk assertions and is green.
    - follow-up visibility slice now materializes Queue 3 advisory summary into read-only observability surfaces:
      - `governance_surface.lifecycle_risk_summary`
      - dashboard governance card `Жизненный цикл памяти` now shows scoped advisory block with `expected_next_state`, `pending_review_risk_7d`, `archive_risk_30d`, `prune_risk_30d`
      - MCP/`observe snapshot` compact summary now surfaces `lifecycle_risk_summary`
    - targeted Rust tests for dashboard/MCP summary rendering are green.
  - follow-up approval slice now materialized as bounded advisory CLI:
    - `cargo run -- memory policy-simulate --project ... --namespace ...`
    - output stays `advisory_only_no_runtime_authority` and maps cohort-risk rows into explicit review recommendations (`review_revalidation_queue / review_archive_candidate / review_prune_candidate / observe_only / hold_current_policy`)
    - protected cohorts stay blocker-visible instead of being silently promoted to automation
  - what is still intentionally pending:
    - measured validation that the recommendation contour improves forgetting/revalidation discipline
    - broader Queue 3 approval/policy workflow beyond this bounded advisory CLI slice
  - current companion proof state:
    - `./scripts/proof_forgetting_consolidation.sh` = green after the policy-simulate slice.
    - `./scripts/proof_observability.sh` = green after the neighboring MCP matrix/dashboard no-data defect was reconciled.
- `Poisson capacity`
  - `materialized_minimal_slice`
  - execution path materialized как Queue 5 forecast-only contour.
  - canonical bounded Queue 5 surface теперь materialized end-to-end:
    - новый module/runtime path:
      - `src/capacity_forecast.rs`
    - новый CLI surface:
      - `cargo run -- observe capacity-forecast --window 5m`
    - scoped observability history now materialized for this contour:
      - `system_snapshot` now persists project-local `_observability.scope_project_code / scope_namespace_code`
      - Queue 5 history source now resolves as `project_scoped_observe_history`
    - first supported queue family for `v1` is surfaced truthfully:
      - `nats_events`
    - supported forecast fields for `v1` now surfaced exactly as planned:
      - `lambda`
      - `expected_arrivals`
      - `poisson_interval_95`
      - `observed_service_rate`
      - `capacity_margin`
    - dashboard now materializes read-only card `Capacity forecast`;
    - `observe snapshot` now materializes separate `capacity_forecast` block;
    - surface contract remains fail-closed:
      - `runtime_authority = false`
      - `routing_authority = false`
      - `truth_authority = false`
      - insufficient sample windows remain explicit, not promoted to success.
  - live runtime state is intentionally window-dependent:
    - latest `system_snapshot` already carries `capacity_forecast`;
    - durable status-truth claim is the scoped history basis:
      - `history_scope.mode = project_scoped_observe_history`
    - measured/insufficient state is not stable proof of completion, because it depends on recent `system_snapshot` density;
    - exact `history_points`, `sample_count`, `lambda`, `capacity_margin` and per-window status values belong to the dated raw snapshot/proof artifact for that run;
    - this keeps Queue 5 `materialized_minimal_slice / forecast-only`, but forbids documenting `5m = measured` or a fixed capacity-quality number as a durable live state until a stable measured-window validation lane exists.
  - current companion proof state:
    - targeted Rust tests for Poisson interval and bucket aggregation are green;
    - targeted Rust tests for Queue 5 dashboard-card fallback and measured-window rendering are green;
    - `./scripts/proof_observability.sh` is green after the 2026-04-25 rerun.
      - the proof now also asserts the `Capacity forecast` dashboard card and `observe snapshot.capacity_forecast` guardrails.
- `regression explain surface`
  - `materialized_minimal_slice`
  - execution path materialized как Queue 4 read-only explain contour.
  - canonical bounded Queue 4 surface теперь materialized end-to-end:
    - новый module/runtime path:
      - `src/regression_explain.rs`
    - новый CLI surface:
      - `cargo run -- observe regression-explain --surface ...`
    - supported outcomes для `v1` now surfaced exactly as planned:
      - `benchmark_pass`
      - `stale_error`
      - `retrieval_helpful`
    - `observe snapshot` теперь materialize-ит отдельный `regression_explain` block;
    - dashboard теперь materialize-ит read-only card `Regression explain`;
    - surface contract fail-closed:
      - `routing_authority = false`
      - `truth_authority = false`
      - `forgetting_authority = false`
      - `insufficient_sample` выводится явно, а не маскируется под success.
  - live runtime state after proof/rerun remains sample-dependent:
    - канонический dashboard после restart уже показывает `Regression explain`;
    - `api/snapshot.regression_explain` now populated;
    - exact sample-pool size, measured/insufficient counters and per-outcome class mix belong to the raw snapshot/proof artifact for that run;
    - when the outcome mix is one-sided, the surface must stay fail-closed with `insufficient_sample` instead of pretending measured regression quality exists;
  - current companion proof state:
    - `./scripts/proof_memory_task_matrix.sh` = green;
    - `./scripts/proof_mcp_task_matrix.sh` = green after strict-heavy contamination preflight and explicit failed-run artifact handling;
    - `./scripts/proof_observability.sh` = green after the 2026-04-25 rerun.
      - the proof now also asserts the `Regression explain` dashboard card and `observe snapshot.regression_explain` guardrails.
  - remaining honest gap:
    - Queue 4 уже production-visible как explain surface, но measured regression quality на live sample пока не достигнута;
    - следующий meaningful gain здесь будет не новый code-path, а накопление двустороннего sample contour, чтобы `insufficient_sample` сменился на measured model output.

### Фундаментальные blocker-ы

На текущий момент фундаментальных blocker-ов к старту scientific execution overlay не зафиксировано.

Есть только нормальные дисциплинарные риски:
- drift между current-state docs и target-state docs;
- попытка перепрыгнуть через очереди `Queue 0-5`;
- попытка кодить без stage gate;
- попытка принять compare-plane или procedural seed за уже завершённый full procedural memory contour;
- попытка трактовать `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` как обзор идей, а не как executable playbook.

### 2026-05-06 — GitHub-first one-command install bootstrap materialized

- закрыт конкретный beta-blocker, из-за которого product install path всё ещё начинался только после ручного `git clone`;
- добавлен [scripts/install_from_github.sh](../scripts/install_from_github.sh):
  - он сам клонирует или обновляет repo;
  - default clone target = `~/.local/share/amai/repo`;
  - затем запускает канонический `scripts/install_amai.sh` уже внутри clone;
  - canonical public source теперь materialized прямо в shell front-door как default `https://github.com/neo-2022/amai.git`, а `--repo-url` остаётся override path для fork/local checkout install.
- добавлен proof [scripts/proof_install_from_github.sh](../scripts/proof_install_from_github.sh) на path `clone -> install -> rerun without duplicate config`.
- добавлен proof [scripts/proof_install_from_github_public_default_source.sh](../scripts/proof_install_from_github_public_default_source.sh) на path `canonical public default source -> clone -> install` без явного `--repo-url`.
- proof bundle для этого install slice:
  - `./scripts/proof_install_from_github.sh`
  - `./scripts/proof_install_from_github_public_default_source.sh`
  - `./scripts/proof_install_auto.sh`

### 2026-05-06 — Install prerequisite front-door fail-closed materialized

- закрыт следующий public-beta blocker после GitHub bootstrap: clean-machine install больше не должен проваливаться слишком поздно и слишком технично при отсутствии локальных prerequisite binaries;
- [scripts/install_amai.sh](../scripts/install_amai.sh) теперь до `cargo run` обязан:
  - fail-closed на отсутствующем `cargo`;
  - fail-closed на отсутствующем `rustc`;
  - для local stack path fail-closed на отсутствующем `docker` или `docker compose`;
  - не требовать `docker` для `--skip-stack` и remote-host onboarding path;
- добавлен hostile proof [scripts/proof_install_prereq_frontdoor.sh](../scripts/proof_install_prereq_frontdoor.sh) на missing `cargo`, missing `rustc`, missing `docker`, missing `docker compose` и safe bypass через `--skip-stack`.

### 2026-05-06 — Compact-chat front-door now rejects unusable clean-surface payloads

- закрыт следующий behavioral beta-blocker в `continuity_compact_chat` shell front-door: API payload больше не считается успехом, если в нём нет реального `chat_start_restore.prompt_text` или нет ни auto-launch команды, ни честного manual clean-surface action;
- [scripts/continuity_compact_chat.sh](../scripts/continuity_compact_chat.sh) теперь fail-closed принимает compact-chat projection только если сохранены:
  - project/namespace identity;
  - `operator_notice.kind`;
  - `handoff.headline` и `handoff.next_step`;
  - непустой `chat_start_restore.prompt_text`;
  - хотя бы один usable clean-surface transition surface: `operator_notice.launch_clean_chat_command` или `operator_notice.required_host_action`;
- hostile proof [scripts/proof_continuity_frontdoor_semantic_invalid_payload_fail_closed.sh](../scripts/proof_continuity_frontdoor_semantic_invalid_payload_fail_closed.sh) расширен отдельными negative-path на missing prompt и missing launch/manual guidance.

### 2026-05-06 — Compact-chat delivery/chat notice now preserves actionable clean-surface fields

- закрыт следующий user-path gap после shell front-door: top-level `delivery_surface_notice/chat_notice` для compact-chat больше не теряет actionable clean-surface fields и не заставляет notice-only consumer разбирать sibling payload вручную;
- [src/observe/observe_control_api.rs](../src/observe/observe_control_api.rs) теперь сохраняет в `compact_chat_delivery_surface_notice_payload` не только `prompt_text/prompt_file/required_host_action`, но и:
  - `launch_clean_chat_command`;
  - `launch_clean_chat_fallback_command`;
  - `launch_clean_chat_command_kind`;
  - `clean_chat_launch`;
  - `manual_fallback_steps`;
- это удержано companion unit-покрытием на source-preserving и blank-fallback ветках notice payload.

### 2026-05-06 — Dashboard compact-chat selector now surfaces actual launch commands

- закрыт следующий compact-chat assist gap: dashboard target selector больше не ограничивается `Auto-launch status` / `Launch bridge`, а показывает и сами actionable clean-surface launch commands;
- [src/dashboard/dashboard_client_budget_support.rs](../src/dashboard/dashboard_client_budget_support.rs) теперь кладёт в selector state:
  - `compact_chat_launch_command`;
  - `compact_chat_launch_fallback_command`;
- [src/dashboard/dashboard_template.html](../src/dashboard/dashboard_template.html) теперь выводит обе команды в compact-chat tooltip как `tooltip-target-picker-command`, а [src/dashboard/dashboard_renderer.rs](../src/dashboard/dashboard_renderer.rs) держит это structural regression guard-ом.

### 2026-05-06 — Compact-chat dashboard assist/toast now includes launch commands

- закрыт ещё один remaining user-path drift: dashboard compact-chat assist/toast раньше включал `note / required_host_action / prompt_file`, но терял actual `launch_clean_chat_command` и `launch_clean_chat_fallback_command`, даже когда notice payload их уже содержал;
- [src/dashboard/dashboard_template.html](../src/dashboard/dashboard_template.html) теперь прокидывает обе команды в `buildCompactChatAssistText(...)` и materialize-ит их в assist text как:
  - `Launch command: ...`
  - `Launch fallback: ...`
- [src/dashboard/dashboard_renderer.rs](../src/dashboard/dashboard_renderer.rs) держит это structural regression guard-ом.

### 2026-05-06 — Compact-chat observe surface now has one final response contract

- закрыт ещё один beta-readiness proof gap: `/api/client-budget-compact-chat` раньше собирал final response inline, и launch/prompt contract держался на разрозненных helper-тестах вместо одного канонического response builder-а;
- [src/observe/observe_control_api.rs](../src/observe/observe_control_api.rs) теперь materialize-ит final payload через `compact_chat_response_payload(...)`, который сводит:
  - compact summary;
  - `delivery_surface_notice`;
  - `chat_notice`;
  - shared launch/prompt/manual-fallback contract;
- там же новый regression `compact_chat_response_payload_keeps_summary_and_notice_launch_contract_aligned` доказывает, что launch commands, command kind, prompt text, thread identity, `clean_chat_launch` и `manual_fallback_steps` доходят до итогового observe surface согласованно, а не теряются между summary и notice lanes.
- companion bundle [scripts/proof_client_clean_chat_launch.sh](../scripts/proof_client_clean_chat_launch.sh) теперь тоже включает:
  - `compact_chat_response_payload_keeps_summary_and_notice_launch_contract_aligned`;
  - `dashboard_html_keeps_compact_chat_assist_path_source_first`;
  так что clean-chat launch contour больше не держится только на helper/runtime unit lanes без broad user-path companion proof.
- final observe response теперь pinned не только для `requested`, но и для оставшихся fail-closed delivery states:
  - `launch_failed`;
  - `available_not_requested`;
  через новые regressions в [src/observe/observe_control_api.rs](../src/observe/observe_control_api.rs), чтобы top-level `chat_notice/delivery_surface_notice` не теряли truth о том, почему clean-surface launch не дошёл до auto-open.
- тем же способом теперь pinned и последние два host-launch verdict-а:
  - `bridge_unavailable`;
  - `disabled_by_policy`;
  так что final response contour закрыт по всей canonical state-линейке `requested / launch_failed / available_not_requested / bridge_unavailable / disabled_by_policy`, а не только по happy-path и одному failure классу.

### 2026-05-06 — Compact-chat success path no longer overclaims seamless launch

- закрыт следующий truth-sensitive defect: simple `code chat` exit-zero больше не трактуется как доказанный seamless clean-surface transition;
- bounded local live-process check показал только `launch command exited zero`, но не дал доказательства новой видимой clean surface в `code --status`, поэтому source contract переведён в fail-closed форму;
- [src/continuity/continuity_compact_chat_helpers.rs](../src/continuity/continuity_compact_chat_helpers.rs) теперь на `requested`:
  - сохраняет `required_host_action` вместо его silent drop;
  - меняет operator notice/message на verification-first wording;
  - больше не говорит, что manual injection шаг уже точно не нужен;
- [src/continuity.rs](../src/continuity.rs) human CLI surface теперь тоже говорит не “уже готово”, а “launch requested, verification still required”, пока live-client proof не materialized;
- companion regressions в [src/continuity.rs](../src/continuity.rs) и [src/observe/observe_control_api.rs](../src/observe/observe_control_api.rs) закрепляют, что `requested` surface сохраняет `required_host_action` и не теряет truthful fallback/verification contract.

### 2026-05-07 — VS Code Codex extension bridge candidate is now pinned as local proof, not folklore

- закрыт ещё один proof gap перед beta-readiness audit: наличие extension-native bridge candidate для clean-surface continuation больше не держится на устном предположении после ручного bundle-reading;
- добавлен [scripts/proof_vscode_compact_chat_extension_bridge.sh](../scripts/proof_vscode_compact_chat_extension_bridge.sh), который на живой локальной машине fail-closed проверяет, что установленный `openai.chatgpt` extension bundle действительно содержит:
  - contributed commands `chatgpt.newChat` и `chatgpt.newCodexPanel`;
  - runtime command registration в `out/extension.js`;
  - internal webview bridge `open-vscode-command` + `shared-object-set composer_prefill` в `use-start-new-conversation-*.js`;
- это proof не повышает claim до seamless UX и не подменяет live-client transition evidence; он лишь закрепляет, что следующий кандидат на честный automatic clean-surface bridge реально существует в установленном extension bundle, а не выдуман из воздуха.

### 2026-05-07 — VS Code external bridge boundary is now pinned as local blocker evidence

- закрыт ещё один beta-readiness proof gap: remaining blocker по VS Code clean-surface path больше не описывается расплывчато как “наверное, внешний bridge ещё не materialized”;
- добавлен [scripts/proof_vscode_compact_chat_external_bridge_boundary.sh](../scripts/proof_vscode_compact_chat_external_bridge_boundary.sh), который на живой локальной машине fail-closed проверяет, что:
  - `code --help` всё ещё не даёт внешнего `--command` front-door для вызова extension commands;
  - установленный `openai.chatgpt` extension `onUri` handler всё ещё маршрутизирует только `uri.path` в `navigateToRoute(path)`;
  - более сильный `composer_prefill` bridge по-прежнему живёт только внутри webview bundle, а не как внешний public launch contract;
- это не “доказательство отсутствия всех возможных путей”, а честное local blocker evidence для текущей поддерживаемой машины и установленного extension bundle.

### 2026-05-07 — Amai public VS Code clean-surface bridge is now materialized

- закрыт крупный beta-blocker: Amai больше не зависит только от upstream route-only `onUri`, internal-only `composer_prefill` или `code chat` command-contract для VS Code clean-surface continuation;
- добавлены:
  - [tools/vscode-amai-bridge/package.json](../tools/vscode-amai-bridge/package.json)
  - [tools/vscode-amai-bridge/extension.js](../tools/vscode-amai-bridge/extension.js)
  - [tools/vscode-amai-bridge/README.md](../tools/vscode-amai-bridge/README.md)
  - [scripts/install_vscode_amai_bridge.sh](../scripts/install_vscode_amai_bridge.sh)
  - [scripts/proof_vscode_compact_chat_public_bridge.sh](../scripts/proof_vscode_compact_chat_public_bridge.sh)
- bridge materialize-ит public `onUri` + command `amaiVscodeBridge.openCleanChat`, читает `prompt_file`, пишет `result_file` truth и открывает clean surface только через public VS Code/Codex commands:
  - `chatgpt.openSidebar`
  - `chatgpt.newChat`
  - `chatgpt.newCodexPanel`
  - `type`
- [src/continuity/continuity_compact_chat_helpers.rs](../src/continuity/continuity_compact_chat_helpers.rs) теперь предпочитает `vscode_uri_amai_bridge`, если bridge установлен и доступен `xdg-open`; старый `code chat --reuse-window` остаётся truthful fallback, а не притворяется единственным automatic path;
- [scripts/onboard_local.sh](../scripts/onboard_local.sh) теперь после `--client vscode` автоматически ставит этот bridge, так что one-command onboarding materialize-ит не только stack/MCP/startup, но и сам clean-surface public bridge;
- [scripts/proof_onboarding.sh](../scripts/proof_onboarding.sh) в этом состоянии снова зелёный, так что install contour не сломан новым bridge install step.

### 2026-05-07 — VS Code public bridge is now live-verified and fail-closed on unverified hosts

- закрыт ещё один truth-sensitive defect: сам факт install/public URI bridge больше не считается достаточным основанием для auto-launch promotion;
- добавлен [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh), который на живом VS Code host:
  - запускает `xdg-open vscode://amai.amai-vscode-bridge/open-clean-chat?...`;
  - ждёт `result_file` от extension;
  - требует финальный `status = launch_requested`, а не промежуточный `launch_started`;
  - при успехе умеет записывать workspace-local verification artifact `.amai/onboarding/vscode-public-bridge-live-state.json`;
- добавлен [scripts/proof_vscode_compact_chat_public_bridge_live.sh](../scripts/proof_vscode_compact_chat_public_bridge_live.sh), который materialize-ит этот live verification artifact как канонический bounded proof lane;
- [src/continuity/continuity_compact_chat_helpers.rs](../src/continuity/continuity_compact_chat_helpers.rs) теперь fail-closed предпочитает `vscode_uri_amai_bridge` только если:
  - bridge установлен;
  - доступен live-verified VS Code URI launcher;
  - существует live verification artifact со статусом `live_launch_verified`;
- без этого marker-а VS Code path больше не притворяется public-bridge-ready и откатывается к truthful `vscode_code_chat_cli` command-contract; install/reinstall bridge теперь ещё и сбрасывает stale live marker через [scripts/install_vscode_amai_bridge.sh](../scripts/install_vscode_amai_bridge.sh).

### 2026-05-07 — VS Code public bridge launcher now bypasses desktop-file mediation

- закрыт ещё один UX defect: canonical `vscode_uri_amai_bridge` launch больше не идёт через общий `xdg-open` mediator, а предпочитает прямой `code --open-url` path;
- live audit на этой машине показал неприятный companion symptom у старого `xdg-open` path: extension действительно активировался по `onUri`, но рядом появлялся побочный `untitled:/.../vscode:/art-local...` dirty surface в renderer logs;
- [src/continuity/continuity_compact_chat_helpers.rs](../src/continuity/continuity_compact_chat_helpers.rs) теперь строит public bridge launch command через `code --open-url`, если `code` binary доступен, и уходит в `xdg-open` только как fallback;
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) синхронно переведён на тот же direct launcher, чтобы proof lane проверял именно канонический runtime path, а не старую desktop-mediated ветку.

### 2026-05-07 — VS Code public bridge now proves no new dirty URI editor growth on the bounded lane

- закрыт ещё один UX defect: bounded live proof для `vscode_uri_amai_bridge` теперь проверяет не только `launch_requested`, но и то, что launch не наращивает известный dirty renderer след `untitled:/.../vscode:/amai.amai-vscode-bridge/open-clean-chat...`;
- [tools/vscode-amai-bridge/extension.js](../tools/vscode-amai-bridge/extension.js) теперь после `handleUri` пытается закрывать transient bridge editor/tabs, если такие surface всё же успевают появиться;
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) теперь считает dirty-surface events до и после live launch и fail-closed режет proof, если счётчик вырос;
- текущий workspace-local artifact [`.amai/onboarding/vscode-public-bridge-live-state.json`](../.amai/onboarding/vscode-public-bridge-live-state.json) на этой машине фиксирует bounded truth `before_count = 2`, `after_count = 2`, то есть канонический direct-launch path не добавил новых dirty events поверх уже исторически существующих следов.

### 2026-05-07 — VS Code public bridge install contour is now version-aware and code-visible

- закрыт ещё один truth-sensitive defect: install contour больше не может молча подменять текущий bridge bundle stale same-version copy и при этом притворяться “installed” только по наличию директории;
- [tools/vscode-amai-bridge/package.json](../tools/vscode-amai-bridge/package.json) теперь version-bumped до `0.0.2`, так что runtime bundle change больше не живёт под старым extension version id;
- [scripts/install_vscode_amai_bridge.sh](../scripts/install_vscode_amai_bridge.sh) теперь удаляет все старые `amai.amai-vscode-bridge-*` directories перед copy/install, а не оставляет version drift в profile;
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) больше не hardcode-ит `0.0.1`, а читает expected version из source `package.json`;
- [scripts/proof_vscode_compact_chat_public_bridge.sh](../scripts/proof_vscode_compact_chat_public_bridge.sh) теперь доказывает не только file presence, но и install/registration truth: после isolated install сам `code --extensions-dir ... --list-extensions --show-versions` обязан видеть точный `amai.amai-vscode-bridge@0.0.2`;
- это не закрывает live UX beta-gap целиком: на уже открытом локальном VS Code окне всё ещё подтверждён отдельный stale running-profile blocker, где старый loaded bridge runtime может не отдать новый `ui_cleanup` result contract без reload/restart.

### 2026-05-08 — VS Code bridge install now rewrites stale registration and exposes a first-class sidebar UI

- закрыт ещё один реальный UI/install defect: bridge больше не зависит от stale `art-local.amai-vscode-bridge-*` registration в `~/.vscode/extensions/extensions.json`, из-за которой уже установленный новый bundle мог не показывать fresh activity-bar/sidebar contributions в живом VS Code окне;
- [tools/vscode-amai-bridge/package.json](../tools/vscode-amai-bridge/package.json) теперь version-bumped до `0.0.3`, а сам bridge materialize-ит полноценный UI-contour: activity-bar container `Amai`, sidebar view `amai.sidebar` и workspace launch commands для sidebar/panel;
- [scripts/install_vscode_amai_bridge.sh](../scripts/install_vscode_amai_bridge.sh) теперь не только sync-ит current bundle в `~/.vscode/extensions/amai.amai-vscode-bridge-<version>`, но и rewrite-ит matching entry в `extensions.json` на exact current path/version и удаляет stale `art-local.amai-vscode-bridge-*` aliases вместо silent compatibility-shim dependence;
- новый bounded proof [scripts/proof_vscode_amai_bridge_registry_sync.sh](../scripts/proof_vscode_amai_bridge_registry_sync.sh) fail-close доказывает этот exact contour: после install `extensions.json` обязан ссылаться на current `amai.amai-vscode-bridge-0.0.3`, а stale `art-local` alias обязан исчезнуть;
- companion UI proof [scripts/proof_vscode_amai_bridge_ui_surface.sh](../scripts/proof_vscode_amai_bridge_ui_surface.sh) доказывает, что source/install bundle реально содержит activity-bar container, sidebar view, icon asset и workspace chat commands, а не только старый `onUri` bridge.
- поверх этого закрыт ещё один user-facing UI defect: activity-bar icon теперь больше не временный bridge glyph, а родной Amai mark, а extension details теперь используют тот же brand mark вместо generic package icon;
- в том же contour закрыт sibling view-contract defect: `amai.sidebar` теперь явно materialize-ится как `type = webview`, так что opened sidebar больше не должен падать в VS Code placeholder `Отсутствует зарегистрированный поставщик данных...`, который относится к tree/data-provider view без registered provider.
- закрыт ещё один direct UX defect в том же sidebar contour: launch buttons больше не зависят от fragile `acquireVsCodeApi()/postMessage/onDidReceiveMessage` chain, а используют direct `command:` URIs under `enableCommandUris`, поэтому клик либо реально запускает `amaiVscodeBridge.openWorkspaceSidebarChat` / `amaiVscodeBridge.openWorkspacePanelChat`, либо fail-close показывает `Amai launch failed: ...` вместо silent no-op.
- после live click probe закрыт и следующий companion runtime defect: sidebar launch path реально падал на прямом `ReferenceError` из-за typo в `collectVisibleSurfaceState`, где result payload ожидал `non_bridge_tab_labels`, а код собирал `nonBridgeTabLabels`; [tools/vscode-amai-bridge/extension.js](../tools/vscode-amai-bridge/extension.js) теперь materialize-ит exact field `non_bridge_tab_labels: nonBridgeTabLabels`, а [scripts/proof_vscode_amai_bridge_ui_surface.sh](../scripts/proof_vscode_amai_bridge_ui_surface.sh) держит structural guard на этот contour, чтобы silent regression не вернулся.

### 2026-05-07 — Stale running-profile bridge runtime now fails closed by explicit version mismatch

- закрыт ещё один truth-sensitive defect: stale уже загруженный VS Code bridge runtime больше не маскируется под generic `ui_cleanup` failure и не требует догадок по косвенному поведению;
- [tools/vscode-amai-bridge/extension.js](../tools/vscode-amai-bridge/extension.js) теперь пишет `public_bridge.version` прямо в `launch_started` / `launch_requested` / `launch_failed` result payload;
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) теперь читает expected version из source `tools/vscode-amai-bridge/package.json` и fail-closed режет live lane отдельным verdict-ом `bridge runtime version mismatch`, если already-loaded VS Code host отвечает старым runtime contract;
- [src/continuity/continuity_compact_chat_helpers.rs](../src/continuity/continuity_compact_chat_helpers.rs) теперь повышает `vscode_uri_amai_bridge` до verified surface только если live marker несёт не просто authority и cleanup truth, а ещё и точную source-matching `public_bridge.version`;
- companion Rust proof теперь прямо закрепляет negative path на missing/wrong runtime version, так что stale runtime больше не может пройти verify path молча.

### 2026-05-07 — Live public bridge proof no longer false-fails on empty dirty-surface scan or exact `false` cleanup state

- закрыт ещё один реальный proof-harness defect: bounded live proof для `vscode_uri_amai_bridge` раньше мог упасть ещё до launch из-за `pipefail`-цепочки в dirty-surface scan, когда `renderer.log` не содержал ни одного bridge match и `xargs rg` возвращал `123` вместо честного `0`;
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) теперь нормализует zero-match dirty-surface path в явный счётчик `0`, а не режет весь proof ложным early-exit;
- в том же verifier закрыт второй truth-sensitive defect: extraction `ui_cleanup.active_editor_matches_bridge_uri_after` больше не использует jq-ветку `// true`, которая превращала корректный `false` в ложный fallback `true` и из-за этого маскировала уже успешный cleanup contract как якобы broken;
- после этих двух фиксов live lane на текущей машине стал реально зелёным: [scripts/proof_vscode_compact_chat_public_bridge_live.sh](../scripts/proof_vscode_compact_chat_public_bridge_live.sh) проходит, а workspace-local artifact `.amai/onboarding/vscode-public-bridge-live-state.json` фиксирует `public_bridge.version = 0.0.2`, `dirty_surface.before_count = 0`, `dirty_surface.after_count = 0`, `ui_cleanup.success = true` и `active_editor_matches_bridge_uri_after = false`.

### 2026-05-07 — Live public bridge proof no longer overclaims stale default-profile registration as runtime truth

- закрыт ещё один truth-sensitive defect: bounded live proof раньше считал default `code --list-extensions --show-versions` authoritative precheck и мог ложно падать на `amai.amai-vscode-bridge@0.0.3`, хотя source bundle, disk install contour и сам живой bridge runtime уже были `0.0.2`;
- прямой runtime probe на этой машине подтвердил корневой факт: default profile registration шумит stale version string, но `code --open-url vscode://amai.amai-vscode-bridge/...` пишет result payload с `public_bridge.version = 0.0.2`, `status = launch_requested` и green `ui_cleanup`;
- закрыт ещё один proof-artifact truth gap: live state для публичного VS Code bridge теперь сохраняет полный `bridge_result` и machine-readable capability drift вместо того, чтобы отбрасывать всё выше `public_bridge.version/ui_cleanup`;
- это материализовало более точную beta-boundary: установленный bridge bundle уже поддерживает `visible_surface`, но текущий loaded runtime на этой машине всё ещё не возвращает `visible_surface` в live result, поэтому higher clean-surface UX proof нельзя честно объявлять закрытым;
- bounded proof для этой границы теперь отдельный и зелёный: [scripts/proof_vscode_compact_chat_visible_surface_runtime_boundary.sh](../scripts/proof_vscode_compact_chat_visible_surface_runtime_boundary.sh).
- закрыт ещё один blocker-sharpening gap над тем же runtime drift: public refresh candidates сами по себе уже не “неизвестный следующий шаг”, а проверенная тупиковая ветка для этой машины;
- новый bounded proof [scripts/proof_vscode_compact_chat_runtime_refresh_boundary.sh](../scripts/proof_vscode_compact_chat_runtime_refresh_boundary.sh) подтверждает, что `code --open-url 'command:workbench.action.restartExtensionHost'` и `code --open-url 'command:workbench.action.reloadWindow'` не снимают `visible_surface` runtime drift: после каждого refresh probe lower live bridge lane остаётся зелёным, но `runtime_capability_drift.visible_surface_missing_from_runtime_result` всё ещё `true`;
- это ещё уже локализует remaining beta-blocker: нужен уже не generic “попробовать reload/restart”, а другой truthful front-door для runtime pickup или новый startup contour, который реально меняет loaded bridge runtime contract.
- [scripts/verify_vscode_compact_chat_public_bridge_live.sh](../scripts/verify_vscode_compact_chat_public_bridge_live.sh) теперь использует disk install truth under `~/.vscode/extensions` plus runtime result version as source of truth и больше не валит live lane только из-за stale default-profile registration drift;
- bounded negative path теперь fail-closed сформулирован честно: verifier режет lane при missing bridge bundle, missing `openai.chatgpt` bundle, disk/source version mismatch или runtime/source version mismatch, но не подменяет эти классы риска шумным default-profile extension listing.

### 2026-05-07 — VS Code bridge install no longer makes the live bundle path disappear during reinstall

- закрыт ещё один lower-layer defect под live UX contour: [scripts/install_vscode_amai_bridge.sh](../scripts/install_vscode_amai_bridge.sh) больше не делает destructive `rm -rf bridge-dir && cp -R ...`, из-за которого running VS Code host уже реально логировал `Unable to read file '~/.vscode/extensions/amai.amai-vscode-bridge-0.0.2/package.json'` и `Extension not found at the location ...` прямо во время reinstall;
- install contour теперь sync-ит source bundle in-place в exact target dir `~/.vscode/extensions/amai.amai-vscode-bridge-<version>`: через `rsync -a --delete` when available и через non-destructive overlay fallback вместо удаления самого target path;
- тот же install contour теперь ещё и сериализован file-lock-ом внутри extensions root, так что concurrent reinstall не может заново смешать cleanup sibling dirs с active sync path;
- old sibling bridge versions всё ещё удаляются после успешного sync, но уже не ценой исчезновения текущего live bundle path из-под running host;
- новый bounded proof [scripts/proof_vscode_amai_bridge_install_live_safe.sh](../scripts/proof_vscode_amai_bridge_install_live_safe.sh) фиксирует именно этот contract: после install `package.json` в exact target dir обязан существовать, а в свежем хвосте текущего `sharedprocess.log` не должен появляться новый missing-manifest error для этого bridge path;
- companion proofs [scripts/proof_vscode_compact_chat_public_bridge.sh](../scripts/proof_vscode_compact_chat_public_bridge.sh) и [scripts/proof_vscode_compact_chat_public_bridge_live.sh](../scripts/proof_vscode_compact_chat_public_bridge_live.sh) остались зелёными после этого safe-install fix, так что lower install contour больше не добавляет noise в текущий bounded live lane.

### 2026-05-07 — Fresh isolated-host clean-surface blocker is now sharply localized to URI delivery

- закрыт ещё один blocker-sharpening proof-gap: remaining fresh-host UX uncertainty больше не размазана по всей clean-surface линии;
- новый bounded proof [scripts/proof_vscode_compact_chat_isolated_host_uri_delivery_boundary.sh](../scripts/proof_vscode_compact_chat_isolated_host_uri_delivery_boundary.sh) поднимает brand-new isolated VS Code host на временных `user-data-dir` и `extensions-dir`, копирует туда `openai.chatgpt` и текущий `amai.amai-vscode-bridge`, ждёт startup, а затем повторно отправляет `code --open-url vscode://amai.amai-vscode-bridge/...`;
- truth boundary на текущей машине теперь конкретная: isolated host уже успевает активировать `openai.chatgpt` в `exthost.log`, но follow-up URI delivery всё ещё не materialize-ит bridge result file за bounded timeout;
- закрыт ещё один sibling blocker-sharpening gap для того же isolated contour: cold one-shot startup path тоже больше не остаётся недоказанной надеждой;
- новый bounded proof [scripts/proof_vscode_compact_chat_isolated_direct_uri_startup_boundary.sh](../scripts/proof_vscode_compact_chat_isolated_direct_uri_startup_boundary.sh) подтверждает, что даже прямой `code --user-data-dir ... --extensions-dir ... --open-url vscode://amai.amai-vscode-bridge/...` на brand-new isolated host не materialize-ит bridge result file за bounded timeout, хотя `openai.chatgpt` activation уже виден в `exthost.log`;
- это ещё уже локализует remaining beta-blocker: проблема уже не в “может быть, isolated host надо стартовать сразу через URI”, а в более глубоком truthful startup/runtime-delivery contour поверх VS Code.
- закрыт ещё один реальный UX-supportability defect в том же proof contour: isolated proof-окна VS Code больше не должны оставаться висеть после завершения проверки и захламлять пользовательскую сессию;
- для этого добавлен targeted teardown helper [scripts/close_vscode_temp_host.sh](../scripts/close_vscode_temp_host.sh), который гасит только temp-host процессы, привязанные к временному `user-data-dir`, а оба isolated proof-скрипта теперь вызывают его через `trap`;
- companion non-regression proof [scripts/proof_vscode_compact_chat_isolated_window_cleanup.sh](../scripts/proof_vscode_compact_chat_isolated_window_cleanup.sh) подтверждает, что после прогона isolated warm/follow-up и cold/one-shot proof pair набор temp `--user-data-dir /tmp/tmp...` VS Code host процессов не увеличивается.
- закрыт ещё один blocker-sharpening gap над оставшимся VS Code startup contour: fallback `vscode_code_chat_cli` тоже больше не остаётся недоказанной надеждой на automation-safe isolated front-door;
- новый bounded proof [scripts/proof_vscode_code_chat_isolation_boundary.sh](../scripts/proof_vscode_code_chat_isolation_boundary.sh) фиксирует текущий CLI contract: `code chat` surface-ит `--profile` и `--new-window`, но не surface-ит `--user-data-dir` и `--extensions-dir`, поэтому этот fallback path на текущем VS Code build нельзя честно использовать как isolated startup proof для beta-ready clean-surface claim.
- закрыт ещё один blocker-sharpening gap над последним оставшимся VS Code fallback path: `vscode_code_chat_cli` сейчас нельзя честно поднять как isolated beta-startup lane;
- новый bounded proof [scripts/proof_vscode_code_chat_cli_isolation_boundary.sh](../scripts/proof_vscode_code_chat_cli_isolation_boundary.sh) подтверждает, что subcommand `code chat` сейчас считает `--user-data-dir` и `--extensions-dir` неизвестными опциями и не пишет `exthost.log` в указанный временный `user-data-dir`;
- это значит, что `vscode_code_chat_cli` пока остаётся только command-contract fallback, а не truthful isolated startup proof path для beta.
- это значит, что следующий beta-blocker уже не “вообще clean-surface UX непонятен”, а гораздо уже: нужен truthful contour для isolated-host URI delivery / fresh-host front-door, после которого можно возвращаться к более высокому visible-surface / startup-restore proof.

### Главная честная незакрытая проблема

Самый важный product gap сейчас такой:
- длинные чаты с несколькими линиями работы ещё не переходят в новую чистую рабочую поверхность полностью бесшовно во всех host/client средах.
- machine-readable и prompt-side restore для multi-line obligations уже materialized;
- compact-chat default/operator path теперь уже сам запрашивает clean chat surface, но больше не притворяется, что одного `launch command exited zero` достаточно для снятия manual verification/fallback шага;
- onboarding/startup artifact path снова согласован с этим contract-ом и не теряет machine-readable startup source-of-truth после reinstall/proof path;
- compact-chat host-launch contour now truthfully surfaces `available_not_requested`, `bridge_unavailable`, `disabled_by_policy`, `requested` and `launch_failed` instead of pretending success when auto-launch is unavailable, policy-gated or command-failed;
- `requested` and `launch_failed` are proof-backed only as opt-in host command-contract states, not proof of seamless client UX;
- новый local extension proof теперь отдельно подтверждает, что в установленном VS Code Codex bundle есть internal bridge candidate (`newChat` / `newCodexPanel` / `composer_prefill`), но этот факт сам по себе всё ещё не равен доказанному внешнему seamless path;
- новый negative boundary proof теперь отдельно подтверждает и противоположную сторону: на текущей машине у upstream VS Code CLI всё ещё нет public `--command` front-door, а upstream extension `onUri` path всё ещё route-only, поэтому bridge пришлось materialize-ить в Amai локально, а не ждать скрытого public surface сверху;
- новый public bridge proof теперь отдельно подтверждает, что Amai уже materialize-ит собственный `vscode_uri_amai_bridge` и auto-installs его на `onboard_local.sh --client vscode`, а новый live proof подтверждает ещё и bounded `launch_requested` path через реальный `xdg-open -> vscode://... -> onUri -> openSidebar/newChat/type`;
- новый public bridge proof теперь отдельно подтверждает, что Amai уже materialize-ит собственный `vscode_uri_amai_bridge` и auto-installs его на `onboard_local.sh --client vscode`, а новый live proof подтверждает bounded `launch_requested` path уже на каноническом direct launcher `code --open-url -> onUri -> openSidebar/newChat/type` без роста known dirty-surface counter;
- compact-chat clean-chat launch support now has a per-client command-contract: VS Code surface-ит `vscode_uri_amai_bridge` только после workspace-local live verification marker, иначе truthful fallback остаётся `vscode_code_chat_cli`; Codex/Hermes/OpenClaw/generic-style clients остаются `manual_only`, пока для них не materialized реальный automatic bridge;
- compact-chat теперь ещё и materialize-ит current client surface и client-specific manual fallback steps для non-VSCode fallback, чтобы manual path был не codex-only и не generic, а привязанным к реальному client surface;
- compact-chat client surface теперь включает и concrete reconnect/open-new-chat assist commands, а не только путь до startup surface;
- compact-chat per-client assist теперь surfaced ещё и прямо в dashboard KPI selector, а не остаётся только в API/operator notice;
- dashboard KPI selector теперь показывает не только manual fallback path, но и текущий auto-launch bridge status / unavailable reason / UX boundary для clean-chat path;
- этот `clean-chat` / `compact-chat` contour относится к delivery/launch surface для новой
  рабочей поверхности и не является ядром памяти `Amai` или отдельным source-of-truth слоем;
- remaining gap теперь сузился ещё уже: bounded bridge launch, no-new-dirty-surface guard, exact version-truth и live public-bridge proof на текущем host уже доказаны; до beta всё ещё нужен более высокий UX proof, что новая clean surface не только получает `launch_requested`, а действительно видимо открывается как нужная рабочая поверхность и что startup restore воспринимается там пользователем без ручного rescue path.
- предыдущий apparent blocker про already-open stale VS Code host на этой машине оказался смешанным с verifier drift: explicit version gate был полезен и truthful, но фактический красный live lane держался ещё и на двух harness-defect-ах. После их исправления текущий loaded host проходит bounded live proof; automatic refresh path для будущих stale-host сценариев остаётся желательным hardening, но это уже не главный доказанный beta-blocker этой машины.
- отдельный code-grounded blocker внутри текущего `compact-chat` shell/observe/dashboard/proof contour после последних фиксов не подтверждён: remaining beta-risk теперь уже не в потерянном contract/state surface, а в отсутствии live-client proof, что clean-surface transition реально бесшовен в поддерживаемых host/client средах.
- как bounded precursor к этому live-proof gap теперь materialized отдельный observe-host launch lane:
  [scripts/proof_host_current_thread_control_uri_launch.sh](../scripts/proof_host_current_thread_control_uri_launch.sh)
  доказывает, что server-side host control launch не только описан в surface, но и реально выполняет `xdg-open` path с `launch_method=xdg_open` и `verification_state=launch_command_executed_exit_zero`, при этом недоступный external launch surface fail-closed отвергается.

Это не должно забываться.
Это один из главных итоговых outcomes всей реализации.

## Fresh update 2026-05-08

- `scripts/remove_amai.sh` больше не является misleading alias к client-only disconnect на Linux managed install path.
- Теперь `remove_amai.sh` автоматически переключает `bootstrap remove` в full uninstall mode, если команда запущена из стандартного GitHub clone `~/.local/share/amai/repo`.
- Full uninstall contour теперь:
  - снимает client config и startup instructions;
  - удаляет локальный `amai.amai-vscode-bridge` bundle для `VS Code`;
  - отключает `systemd --user` unit `amai-stack.service`;
  - делает `docker compose --profile monitoring down --remove-orphans --volumes`;
  - удаляет runtime tree и managed clone root.
- `bootstrap disconnect` при этом сохранён как узкий client-only path и остаётся каноническим выбором для случаев, где install/runtime нужно оставить живыми.
- Новые proofs:
  - `cargo test --quiet remove_vscode_bridge_install_removes_bundle_and_registry_entries`
  - `./scripts/proof_remove_amai_full.sh`
- Live laptop verification for this contour closed the old truth gap: раньше one-command uninstall был ложным claim, потому что `remove_amai.sh` снимал только client config; теперь full remove materialized, а cleanup verified по unit/service/container/volume/extension/runtime слоям.
- закрыт ещё один install/autostart defect под laptop lifecycle contour: `scripts/install_stack_autostart.sh` больше не пишет `amai-stack.service` прямо в финальный путь, из-за чего прерванный install мог оставить обрезанный user-unit без `ExecStart` и ломать следующий one-command install через `Loaded: bad-setting`;
- unit render теперь идёт через temporary file + atomic `mv`, а `scripts/proof_stack_autostart.sh` дополнительно стартует из уже повреждённого preexisting `amai-stack.service` и проверяет, что rerender восстанавливает полный unit contract вместо silent reuse битого файла.

## Что агент должен делать прямо сейчас

Если агент подключился сегодня, ему не надо перечитывать всё подряд, чтобы понять общий статус.

Он должен:
1. Прочитать этот файл.
2. Увидеть:
   - что baseline уже есть;
   - что Stage 1-9 закрыты;
   - что Stage 10 снова fresh-green после clean rerun `proof_mcp_task_matrix.sh` и `proof_observability.sh`;
   - что работать дальше надо по лестнице, а не по самой заметной user-facing поверхности.
3. Сначала определить, к какому уровню относится текущая workline:
   - foundation truth / memory / continuity / task / restore;
   - delivery / startup / reconnect / clean work surface;
   - proof/scientific overlay;
   - surface/dashboard wording.
4. Если работа не упирается в уже изменённый нижний слой или в явную truth-путаницу, нельзя
   начинать с surface/dashboard линии.
5. После этого открывать:
   - `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md` как canonical Stage-лестницу;
   - `docs/IMPLEMENTATION_GATES.md` как proof/gate contract;
   - `docs/AMAI_TASK_TREE_PLAN.md`, если работа про task/commitment/restore foundation;
   - `docs/AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` только если workline действительно дошла до
     overlay-слоя.

Простое правило:
- queue-first discipline остаётся обязательной внутри scientific contour;
- но scientific contour не имеет права вытеснять foundation-first порядок всего проекта;
- dashboard/surface работа не должна снова становиться отдельной бесконечной линией, пока не
  изменился нижний слой или не найдено явное искажение истины.

## Обязательный закон обновления этого файла

После каждого значимого шага этот файл обязан обновляться.

Минимум что надо обновить:
- `Что уже точно сделано`;
- `Что сейчас в работе`;
- `Что ещё не materialized`;
- `Ближайший следующий этап`;
- `Фундаментальные blocker-ы`, если они появились или исчезли.

И ещё 2 обязательных действия рядом:
- записать новый `continuity handoff`;
- обновить профильный документ, если изменился не только статус, но и сам контракт.

Если меняется сама каноническая лестница реализации или разрешённый порядок workline-ов, рядом
обязательно обновить и pointer-only ссылки на неё в:
- `README.md`;
- `docs/AGENT_START_HERE.md`;
- `docs/AMAI_SYSTEM_OVERVIEW.md`.

Это нужно затем, чтобы верхние входные документы не превращались в тихо устаревшие вторичные
описания порядка работ.

Простое правило:
- если этот файл не обновлён, агент снова будет вынужден “вычислять статус проекта” по косвенным признакам;
- это считается ошибкой внедрения, а не нормой.
