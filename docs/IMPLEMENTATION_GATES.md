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

Текущая status-truth оговорка:
- для уже отмеченных Stage 0-10 checkbox-ов отсутствие этих external harnesses не должно молча превращаться в fake-green external benchmark maturity;
- пока registry/harness/source/real-runtime/real-score/upstream-parity lane не materialized, любой claim вида `external benchmark-grade long-term memory` обязан маркироваться как `proof-refresh-required` или `external-maturity-gap`;
- если fresh audit показывает, что конкретный stage был закрыт только на внутреннем fixture/proof bundle при обязательном external benchmark requirement, `IMPLEMENTATION_STATUS.md` обязан явно отделить `internal-stage-closed` от `external-benchmark-maturity-not-proven`;
- если fresh external harness после materialization падает, соответствующий stage claim снимается до root-cause, fix и повторного proof.

Итог:
- внутренний stage checkbox не заменяет external memory benchmark maturity;
- external registry не является декоративным roadmap пунктом;
- команда должна materialize-ить его как отдельный proof contour или честно держать gap открытым.

Fresh status 2026-04-24:
- `./scripts/proof_memory_external_benchmarks.sh` теперь является proof для `download + adapter workspace + normalized case preparation + synthetic runtime/score smoke`, а не для полного scored benchmark verdict;
- LongMemEval `longmemeval_s_cleaned`, MemoryAgentBench (`accurate_retrieval`, `conflict_resolution`, `long_range_understanding`, `test_time_learning`) и LoCoMo `locomo10` обязаны иметь non-empty normalized cases with question/context/id before proof can pass;
- LoCoMo converter должен раскрывать `qa[]` в отдельные cases и рендерить `conversation.session_*` as context; прежнее состояние `10 cases / missing question+context+answer` считается fixed proof-harness defect;
- AMA-Bench now has proof-backed normalized cases and bounded runtime+baseline-score evidence from the manual HF dataset install; full dataset runtime and benchmark-grade maturity remain `external-maturity-gap`;
- external memory benchmark-grade maturity остаётся не доказанной, пока не пройден полный real benchmark runtime+score contour и, где применимо, official upstream scorer parity.

Fresh refresh 2026-04-25:
- `./scripts/proof_memory_external_benchmarks.sh` rerun green in the same bounded lane; it did not add real benchmark predictions, real scored outputs or upstream scorer parity.
- MemoryAgentBench prep is operationally heavy: `accurate_retrieval` produced multi-GiB normalized/request artifacts for 2000 cases. Future stage-close or regular benchmark automation must account for artifact size, runtime and cleanup separately from correctness.

Fresh bounded real runtime 2026-04-26:
- `./scripts/proof_memory_external_real_bounded.sh` materializes bounded, dataset-specific LongMemEval execution evidence: limited `longmemeval_s_cleaned` normalized cases, real `external-memory-run` predictions, runtime status/case metrics and `external-memory-score` baseline output.
- `./scripts/proof_memory_external_real_bounded_ama_bench.sh` materializes bounded, dataset-specific AMA-Bench execution evidence: limited `ama_bench_manual` normalized cases, real `external-memory-run` predictions, runtime status/case metrics and `external-memory-score` baseline output, while keeping `official_scorer_boundary.source_kind=official_scorer_contract_unavailable`.
- `./scripts/proof_memory_external_real_bounded_memoryagentbench.sh` materializes bounded, dataset-specific MemoryAgentBench execution evidence: limited `memoryagentbench_conflict_resolution`, `memoryagentbench_long_range_understanding`, and `memoryagentbench_test_time_learning` normalized cases, real `external-memory-run` predictions, runtime status/case metrics and `external-memory-score` baseline output, while keeping `official_scorer_boundary.source_kind=official_scorer_contract_unavailable`. Fresh reruns also materialize and bounded-proof-check the joint invariant `top_ranked_relevance_and_gold_answer_supported_retrieval_cases <= top_ranked_gold_answer_supported_retrieval_cases` for these named profiles, and now fail-closed on explicit boolean-typed `runtime_corpus_sha256` / `runtime_corpus_reused_from_previous_case` fields. On the current default bounded limit (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`), the bounded truth is profile-specific: `conflict_resolution` and `test_time_learning` must show one corpus hash with `2/3` reused cases and `index_project_ms=0` on the reused cases, while `long_range_understanding` must show three distinct corpus hashes with `0/3` reused cases. This is a bounded proof requirement for those datasets, not a general promotion gate or cache maturity claim.
- `./scripts/proof_memory_external_real_bounded_memoryagentbench_accurate_retrieval_blocked.sh` materializes the bounded `memoryagentbench_accurate_retrieval` slice as a still-blocker-visible contour, but the benchmark-specific shaping blocker is now gone in the bounded run. On the current default bounded limit (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`), the slice carries perfect baseline answers plus proxy evidence of retrieval participation and relevance: `top_ranked_relevant_retrieval_cases=3/3`, `gold_answer_supported_retrieval_cases=3/3`, `top_ranked_gold_answer_supported_retrieval_cases=3/3`, `top_ranked_relevance_and_gold_answer_supported_retrieval_cases=3/3`, `top_ranked_structural_fact_supported_cases=3/3`, `benchmark_specific_query_override_cases=0`, `benchmark_specific_answer_extraction_cases=0`, `benchmark_specific_shaping_present=false` and `generic_runtime_maturity=true`. The runtime also now proves identical-corpus reuse on this default fixed slice: `runtime_corpus_unique_sha_count=1`, `runtime_corpus_reused_cases=2`, reused cases keep `index_project_ms=0`. The bounded latency story has also advanced further: after correcting the synthetic-runtime edge-cache skip gate to match the actual `.md` runtime corpus shape, the current bounded run now materializes cold first-case `index_project_ms` at about `0.87s`, bounded `index_project_ms.avg` at about `0.29s`, and bounded `total_case_ms.avg` at about `0.91s`. `latency_maturity=false` still stays blocker-visible, but no longer because cold indexing dominates the slice average; the remaining latency blocker is simply that this is not a latency-grade contour, only a fixed bounded slice. This reuse is intentionally narrow and fail-closed: it is same-process, same-run, byte-identical materialized runtime-corpus reuse guarded by the current `paths.txt` materialization, not a persistent cross-run cache, not a claim about hidden environment dependencies, and not semantic equivalence. That means the bounded slice is now generic on the runtime/query/extraction side, has no bounded top-rank gap on the current proxy surfaces, and now also carries a stronger `anchored_fact_shape_proxy` contract for the top-ranked snippet on this fixed three-case slice. It still must not be treated as fully trusted bounded-runtime maturity because both the query-overlap layer and the anchored fact-shape layer remain non-semantic proxies, benchmark-grade scorer parity is still absent, and the latency contour is improved but still bounded-only. The same proof writes `bounded-proof-contract.json`, and that contract must keep `benchmark_grade_maturity=false`, `official_upstream_scorer_parity=false`, `answer_source_rate_semantic_proof=false`, `latency_maturity=false` and blocker-visible semantic/scorer reasons. The script now also re-runs targeted Rust negative tests for malformed/missing resume identity fields and for the `paths.txt`/same-hash reuse gate, so the bounded contour no longer relies on a happy-path-only launcher story for those reject paths; those tests prove reject-path behavior, not a broader filesystem-stability guarantee.
- `./scripts/proof_qdrant_postgres_failure_contract.sh` materializes the cross-store `Qdrant/Postgres` recovery-contract contour as a standalone harness instead of leaving it implicit inside the bounded MemoryAgentBench lane. It must keep the four branch classes explicit and green via targeted Rust tests: `before_qdrant_update` must surface `consistency_state=postgres_failure_before_qdrant_update` with `required_action=retry_or_investigate_postgres_before_retrying_qdrant_mutation`; compensated rollback branches must surface `cross_store_consistency_restored_by_compensation` with `no_further_cross_store_recovery_required`; `commit_outcome_unknown` branches must surface `cross_store_consistency_unknown_commit_outcome` with `manual_cross_store_investigation_required`; compensation-failure branches must surface `cross_store_inconsistent_after_compensation_failure` with the same manual-investigation action. The harness now also requires an emitter-level observability test for the live failure-verdict stage `index_project.qdrant_postgres_failure_verdict`, a real forced runtime test that enters `index_project_file_under_lock`, and proof that every `manual_cross_store_investigation_required` branch writes a durable remediation bundle instead of leaving operator recovery as a log-only hint. For the existing-document compensation-failure runtime lane that means `failure_mode=existing_document_inconsistent_state`, `compensation_ok=false`, and a surfaced remediation bundle path on the observability lane. It writes `tmp/qdrant-postgres-failure-contract/proof-contract.json`; use that artifact as the machine-readable disclaimer for explicit branch mapping, emitted observability fields, the forced runtime seam scope, the remediation-bundle requirement, and the fact that this remains forced-failure proof rather than distributed-transaction or crash-safe runtime maturity. This harness proves the explicit recovery contract, the shared log rendering for forced failure branches, one truthful runtime orchestration path, and the presence of a durable operator handoff artifact; it is not a distributed-transaction maturity claim and does not prove crash-safe or semantic benchmark behavior by itself.
- This proof explicitly remains `bounded_real_runtime_score`, not full external benchmark-grade maturity: it is limited by `AMAI_EXTERNAL_MEMORY_REAL_LIMIT`, uses Amai baseline scoring, and still lacks official upstream scorer parity.
- Runtime metrics must show at least one retrieval-backed case after relaxed benchmark-query retry; fallback-only real predictions are not accepted as retrieval/runtime evidence.
- Runtime metrics must include `answer_source_boundary.boundary_version = external_memory_answer_source_boundary_v1`; `retrieval_answer_cases > 0` is required, while `semantic_precision_maturity` stays `false` until retrieval-answer coverage and semantic relevance are separately validated.
- Runtime metrics must include `retrieval_relevance_boundary.boundary_version = external_memory_retrieval_relevance_boundary_v1`; `judge_kind=query_overlap_proxy`, `relevant_retrieval_evidence_cases > 0`, and `top_ranked_relevant_retrieval_cases <= relevant_retrieval_evidence_cases` are required, while `semantic_precision_maturity` stays `false` until gold-labeled semantic relevance is integrated.
- Runtime metrics must include `gold_answer_relevance_boundary.boundary_version = external_memory_gold_answer_relevance_boundary_v1`; `judge_kind=gold_answer_lexical_overlap`, `label_source_kind=benchmark_answer_field`, non-empty `gold_labeled_cases`, `gold_answer_supported_retrieval_cases <= retrieval_evidence_cases`, and `top_ranked_relevance_and_gold_answer_supported_retrieval_cases <= top_ranked_gold_answer_supported_retrieval_cases` are required, while `semantic_precision_maturity` stays `false` because this is lexical answer-support accounting, not semantic/upstream scorer parity. Here `top_ranked_relevance_and_gold_answer_supported_retrieval_cases` means only cases whose top-ranked snippet passes both the bounded relevance proxy threshold and the lexical gold-answer support test; if top-ranked gold support exists without proxy relevance, blocker `top_ranked_gold_answer_support_without_relevance_proxy` must stay surfaced. A zero `gold_answer_supported_retrieval_cases` count is acceptable only as explicit blocker-visible evidence that the bounded retrieval did not surface answer-bearing snippets.
- For bounded `memoryagentbench_accurate_retrieval`, runtime metrics must also include `structural_fact_relevance_boundary.boundary_version = external_memory_structural_fact_relevance_boundary_v1`; `judge_kind=anchored_fact_shape_proxy`, `proxy_applicable_cases > 0`, and `top_ranked_structural_fact_supported_cases <= proxy_applicable_cases` are required. This is a stronger bounded contract than raw query overlap because it checks whether the top-ranked snippet structurally expresses the asked fact shape, but it is explicitly limited to those structural fact question shapes. It does not redefine the benchmark name, does not prove generic retrieval accuracy outside that shape, still keeps `semantic_precision_maturity=false`, and must stay blocker-visible as `anchored_fact_shape_proxy_not_semantic_judgment`.
- For bounded `memoryagentbench_accurate_retrieval`, proof must also keep the human-visible winner preview aligned with the support verdict: when `retrieval_payload_top_ranked_gold_answer_supported=true` on the bounded winner, `retrieval_payload_top_ranked_preview` must surface the answer-bearing span rather than an arbitrary leading excerpt. This is an explainability contract only, not a semantic-maturity gate.
- Relaxed benchmark-query retry is recall recovery evidence only. Query-overlap and gold-answer lexical overlap relevance are bounded proxies only. Neither proves semantic precision, answer relevance or upstream scorer parity.
- `external-memory-score.evidence_boundary.boundary_version` must be version-pinned; semantic changes to the bounded score boundary require proof/runbook updates.
- `external-memory-score.evidence_boundary.maturity_blocking_reasons` must remain explicit while the scorer is baseline-only and full dataset/upstream parity lanes are absent.
- `external-memory-score.official_scorer_boundary.boundary_version` must be version-pinned as `external_memory_official_scorer_boundary_v1`; for LongMemEval it records the official upstream scorer contract (`evaluate_qa.py`, `print_qa_metrics.py`, `gpt-4o-2024-08-06`) while keeping `official_upstream_scorer_parity=false`.
- `external-memory-score.official_scorer_boundary.official_prompt_templates_embedded=false` is intentional for this slice: the boundary records input/output/model/metric contract only, not prompt-template parity.
- `external-memory-score.official_scorer_boundary.maturity_blocking_reasons` must keep `live_official_llm_judge_not_run`, `official_eval_log_not_materialized`, `official_upstream_metrics_not_materialized` and `official_prompt_templates_not_embedded` visible until the live official scorer lane is run and reconciled.
- `external-memory-official-judge.boundary_version` must be version-pinned as `external_memory_official_judge_execution_v1`; this lane embeds the upstream LongMemEval prompt templates and may write eval-results JSONL only when `--allow-live`, the official `gpt-4o-2024-08-06` model and the configured API key env are present. Its default proof must stay fail-closed without writing a synthetic live log.
- `./scripts/proof_memory_external_official_judge.sh` must cover no-live, missing-key and non-official-model blockers before this lane can be cited as materialized.
- `./scripts/proof_memory_external_official_judge_api_failure.sh` must cover local API-failure blockers for simulated HTTP 429, HTTP 5xx, HTTP 200 response-contract violations and connection-refused transport responses: summary status stays `blocked`, `official_judge_live_execution_failed` plus the classified failure are visible, eval-results JSONL is absent, and the configured dummy key value is not written to the summary, including hostile fake error bodies that echo the key.
- `./scripts/proof_memory_external_official_judge_live_bounded.sh` is the bounded live-operator guard: without the configured API key it must prove `official_judge_api_key_not_materialized` and no eval log; with the key it must materialize the bounded official eval log, verify provenance fields do not persist the key value, and run `external-memory-official-score` on that log.
- Missing-key/offline official judge summaries, including the default proof plus bounded and balanced live-operator summaries, must not contain `[REDACTED_OFFICIAL_JUDGE_API_KEY]`; the marker is allowed only in hostile echo-key or key-backed response redaction paths where a materialized key value was actually removed.
- This no-key/offline hygiene guard validates artifact cleanliness only. It does not prove live/API-dependent judge execution, upstream scorer parity, or benchmark-grade maturity.
- Key-backed live-operator proofs must also run the Rust `external-memory-secret-scan` verifier across every regular file in the proof output directory and fail closed on any configured API key value match. This check verifies absence of the secret value, not merely `api_key_value_persisted=false` metadata, and prevents unlisted intermediate artifacts from bypassing the guard.
- `./scripts/proof_memory_external_official_judge_live_balanced.sh` is the six-type bounded guard: it must select one raw `longmemeval_s_cleaned` record for each official question type, normalize through `external-memory-prepare --source-path`, run six Amai runtime predictions, and then enforce the same live/no-key official judge boundary. This is required before citing bounded live evidence as capable of a full official-score reconciliation.
- `external-memory-official-score.boundary_version` must be version-pinned as `external_memory_official_score_reconciliation_v1`; it reconciles upstream-style LongMemEval eval-results JSONL only and must not convert a synthetic or manually supplied log into a live scorer parity claim.
- `./scripts/proof_memory_external_official_score_reconcile.sh` must cover the reconciled path, missing eval-log path and invalid eval-log path before this lane can be cited as materialized.

Accepted evidence boundary для этого registry:
- `external-check` = source/tool preflight only;
- non-empty normalized cases + clean manifest = prep lane only;
- synthetic `external-memory-run/score` exact-match smoke = command-contract only;
- `external-memory-official-judge` default/offline proof = live-judge execution/log lane contract plus fail-closed blockers only; scorer parity still requires a real live run, reconciliation and full dataset runtime evidence;
- local official judge API-failure proof = deterministic fake-provider contract only; it must not be cited as live provider evidence or benchmark-grade maturity;
- bounded live official judge proof = operator lane for the current `longmemeval_s_cleaned` bounded artifacts only; no-key success is a secret-gate proof, while key-backed success remains bounded and does not close full dataset/upstream parity;
- balanced six-type live official judge proof = bounded official-question-type coverage and source-path normalization proof only; it avoids first-N sample type gaps but still does not prove distributional representativeness, full runtime, or upstream parity;
- synthetic `external-memory-official-score` eval-log reconciliation = official log schema/metric contract only;
- bounded real `external-memory-run/score` proof = named dataset/limit runtime+baseline-score evidence plus answer-source accounting only, not full retrieval precision or benchmark maturity;
- empty `cases.jsonl`, `total=0` manifest, manual marker or stale `stage=running` status = no closure evidence;
- full maturity = real dataset predictions + real scored outputs + upstream scorer parity.

Current consensus record ids:
- `AMAI-AUDIT-EXTMEM-001`: internal stage closure split from external benchmark maturity;
- `AMAI-AUDIT-EXTMEM-002`: LoCoMo normalized-case prep verified, full maturity still open;
- `AMAI-AUDIT-EXTMEM-003`: AMA-Bench bounded prep/runtime/score evidence is materialized, but full dataset and benchmark-grade maturity remain open;
- `AMAI-AUDIT-EXTMEM-004`: runtime/score CLI is synthetic command-contract smoke until real benchmark runs exist.
- `AMAI-AUDIT-EXTMEM-005`: bounded, dataset-specific LongMemEval real runtime+baseline-score proof exists; full-dataset, semantic retrieval precision and upstream-parity maturity stay open.

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

## Team consensus protocol для docs-vs-code audit

Если задача требует проверить, что документация глубоко соответствует реализации, один агент не имеет права закрывать finding только своим впечатлением.

Для каждого найденного недостатка нужен consensus record:
- `claim_owner`: фиксирует исходный doc claim, путь, строку/heading и точную формулировку статуса;
- `implementation_verifier`: указывает code/schema/runtime surface, который должен реализовывать claim;
- `proof_owner`: указывает proof/verify/dashboard/raw lane и свежесть результата;
- `consensus_verdict`: одно из `verified_working`, `internal_closed_proof_refresh_required`, `partial`, `not_materialized`, `failing`, `stale_doc_claim`;
- `required_doc_action`: `keep`, `downgrade`, `remove_checkbox`, `add_gap`, `move_to_roadmap`, `split_internal_vs_external_maturity`;
- `required_implementation_action`: exact next implementation/proof step, если claim не подтверждён.

Правило принятия:
- если verifier и proof_owner не могут показать реализацию и свежий proof, claim нельзя оставлять как `работает / green / доказано`;
- если code surface есть, но proof устарел или не покрывает negative path, status = `internal_closed_proof_refresh_required`;
- если proof показывает failing path, status = `failing`, checkbox снимается, а docs обязаны описать root-cause и recovery plan;
- если claim относится только к design/roadmap, он должен жить в roadmap как `planned / required`, а не в baseline/status как `materialized`;
- если claim покрыт внутренним harness, но не внешним benchmark registry, он должен явно разделять `internal-stage-closed` и `external-benchmark-maturity-not-proven`.

Этот protocol обязателен для:
- audit work по `IMPLEMENTATION_STATUS.md`;
- любого снятия или постановки stage checkbox;
- claims про `closed`, `green`, `materialized`, `real works`, `verified`, `production-visible`;
- claims, которые surfaced в README, MCP summaries, dashboard или startup/preflight machine-readable artifacts.

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

Fresh proof-refresh status 2026-04-24:
- `proof_hostile.sh` and `proof_memory_task_matrix.sh` passed;
- `proof_benchmark_contamination_preflight.sh` passed, including the strict-heavy fail-closed path;
- `proof_mcp_task_matrix.sh` passed after strict-heavy contamination preflight and explicit failed-run artifact handling;
- `proof_observability.sh` passed after dashboard `MCP task matrix compare` was revalidated from fresh matrix snapshots.

Regression contract behind the restored green Stage 10 claim:
- latency-sensitive benchmark proofs must detect or isolate heavy unrelated local-model runners instead of silently producing contaminated SLA verdicts;
- MCP task matrix failure must publish or preserve an explicit red-state artifact for observe/dashboard surfaces, not erase the compare lane into no-data;
- only a clean rerun of the full Stage 10 bundle may keep the public claim `governance/safety/evaluator loop fresh green`.

Scientific Queue 4/5 status-truth boundary:
- `proof_observability.sh` may prove the Queue 4/5 dashboard cards and raw snapshot contracts are present and fail-closed;
- this proof does not prove measured regression quality or durable measured capacity quality;
- Queue 4 accepted statuses in raw proof remain `measured | insufficient_sample | not_materialized`;
- Queue 5 accepted window statuses in raw proof remain `measured | insufficient_sample`;
- exact live values such as `history_points`, `sample_count`, `lambda`, `capacity_margin`, sample pool size and per-outcome class mix must be cited only from the raw artifact of the current run;
- documentation must keep `AMAI-AUDIT-SCI-Q45-001..003` separate from any future measured-quality claim.

Fresh refresh 2026-04-25:
- `./scripts/proof_observability.sh` passed and still validates Queue 4/5 read-only guardrails rather than measured scientific quality.
- The raw snapshot is volatile by design. The 2026-04-25 spot-check observed Queue 5 `history_points=44`, `nats_events.1m=measured`, `nats_events.5m=insufficient_sample`, `sample_count=3`, while Queue 4 remained `measured_outcomes=0`, `insufficient_sample_outcomes=3`, `sample_pool_size=31`. These numbers are evidence for that run only.

Host-side clean-chat / client startup boundary:
- `proof_mcp_orphan_cleanup.sh`, `proof_client_reconnect.sh` and `proof_client_clean_chat_launch.sh` prove orphan MCP cleanup with spaced `amai mcp serve` argv0, client install/remove, reconnect assist, VS Code clean-chat launch command-contract and non-VSCode manual-only boundary, not fully seamless clean-chat migration;
- `proof_vscode_compact_chat_extension_bridge.sh` proves a narrower precursor only: the installed VS Code Codex extension bundle really contains `chatgpt.newChat`, `chatgpt.newCodexPanel`, and the internal `composer_prefill` bridge path in `use-start-new-conversation-*`; this is existence evidence for an extension-native bridge candidate, not seamless user-path proof;
- `proof_vscode_compact_chat_external_bridge_boundary.sh` proves the old negative boundary on the live upstream bundle itself: `code --help` still exposes no external `--command` front-door, the installed `openai.chatgpt` extension `onUri` handler still routes only `uri.path` into `navigateToRoute(path)`, and the stronger `composer_prefill` bridge is still internal-only inside the webview bundle; this is blocker-sharpening evidence for upstream/public surfaces, not a seamless-host claim;
- `proof_vscode_compact_chat_public_bridge.sh` now proves that Amai materializes its own public VS Code bridge as a repo-local extension install contour: `scripts/install_vscode_amai_bridge.sh` installs `amai.amai-vscode-bridge`, the bridge contributes `amaiVscodeBridge.openCleanChat`, exposes public `onUri`, and drives only public VS Code/Codex commands (`chatgpt.openSidebar`, `chatgpt.newChat`, `chatgpt.newCodexPanel`, `type`) plus result-file truth for launch status; this upgrades the blocker from “missing public bridge” to “missing live client UX proof that the bridge really opens a clean surface and lands restore end-to-end”;
- the same `proof_vscode_compact_chat_public_bridge.sh` now also proves install/registration truth instead of only file presence: after installing into an isolated `extensions_dir`, `code --extensions-dir ... --list-extensions --show-versions` must surface the exact current `amai.amai-vscode-bridge@<version>` bundle, so version drift or copy-only false positives fail closed before live UX claims;
- `proof_vscode_amai_bridge_install_live_safe.sh` now proves the lower install contour does not reintroduce the old live reinstall race on the running local VS Code host: `scripts/install_vscode_amai_bridge.sh` must leave `~/.vscode/extensions/amai.amai-vscode-bridge-<version>/package.json` present after install and must not append a fresh `Unable to read file .../amai.amai-vscode-bridge-<version>/package.json` error to the current `sharedprocess.log`;
- `proof_vscode_amai_bridge_registry_sync.sh` now proves the same install contour also normalizes stale extension registration instead of leaving UI contributions pinned to the old `art-local.amai-vscode-bridge-*` alias: after install, `extensions.json` must point the `amai.amai-vscode-bridge` entry at the exact current `amai.amai-vscode-bridge-<version>` bundle and the stale `art-local` alias must be removed;
- `proof_vscode_compact_chat_public_bridge_live.sh` now proves the stronger bounded live lane on a real VS Code host: direct `code --open-url vscode://amai.amai-vscode-bridge/open-clean-chat?...` reaches the bridge, the bridge reads `prompt_file`, executes `chatgpt.openSidebar` + `chatgpt.newChat` + `type`, writes `launch_requested`, records workspace-local verification artifact `.amai/onboarding/vscode-public-bridge-live-state.json`, and must not increase the known dirty-surface renderer counter for `untitled:/.../vscode:/amai.amai-vscode-bridge/open-clean-chat...`;
- `proof_vscode_compact_chat_isolated_host_uri_delivery_boundary.sh` now proves the sharper opposite boundary for fresh-host UX work: a brand-new isolated VS Code host with temporary `user-data-dir` and copied `openai.chatgpt` + `amai.amai-vscode-bridge` bundles does reach `openai.chatgpt` activation in `exthost.log`, but a follow-up `code --open-url vscode://amai.amai-vscode-bridge/...` still does not materialize the bridge result file in that isolated host within the bounded timeout; this keeps the remaining beta blocker localized to isolated-host URI delivery instead of vague clean-surface UX uncertainty;
- `scripts/proof_vscode_compact_chat_isolated_direct_uri_startup_boundary.sh` now proves the sibling cold-start boundary for the same contour: even one-shot `code --user-data-dir ... --extensions-dir ... --open-url vscode://amai.amai-vscode-bridge/...` on a brand-new isolated host still does not materialize the bridge result file within the bounded timeout, while `openai.chatgpt` activation is present in `exthost.log`; this removes the false hope that the missing pickup path is simply “launch the URI as the first startup action”;
- the isolated-host proof pair must now also be non-leaking: temporary VS Code proof windows are closed on exit through a dedicated targeted teardown keyed by the temporary `user-data-dir`, and `scripts/proof_vscode_compact_chat_isolated_window_cleanup.sh` fail-closes if running the isolated warm/follow-up and cold/one-shot proofs leaves behind a different set of temp `--user-data-dir /tmp/tmp...` VS Code host processes;
- `scripts/proof_vscode_code_chat_isolation_boundary.sh` now proves the sibling CLI boundary for the fallback `vscode_code_chat_cli` path: the current `code chat` subcommand still exposes `--profile` and `--new-window`, but does not surface `--user-data-dir` or `--extensions-dir` in its supported help contract, so this fallback cannot be promoted into an automation-safe isolated startup/front-door proof on this VS Code build;
- `scripts/proof_vscode_code_chat_cli_isolation_boundary.sh` now proves the sibling CLI boundary for the remaining VS Code fallback path: `code chat` currently treats `--user-data-dir` and `--extensions-dir` as unknown subcommand options, and an isolated temp `user-data-dir` therefore does not receive `exthost.log`; this means `vscode_code_chat_cli` cannot currently serve as the clean isolated beta-startup proof lane for VS Code.
- that same live lane is now version-truth-sensitive: the bridge runtime must report the exact current `public_bridge.version` from the source bundle, otherwise the proof fails closed with an explicit `bridge runtime version mismatch` verdict instead of silently treating a stale already-loaded VS Code host as “cleanup missing” noise;
- the same live lane must treat disk install truth and runtime truth as authoritative over default-profile registration noise: verifier prechecks may read `~/.vscode/extensions/<bundle>/package.json` plus the bridge result payload, but must not fail solely because `code --list-extensions --show-versions` on the default running profile surfaces a stale bridge version unrelated to the source bundle or loaded runtime;
- the same live verifier must also treat two bounded negative paths truthfully instead of red-by-harness: zero dirty-surface matches in `renderer.log` are a legitimate `0`, not a `pipefail/xargs` early-exit, and `ui_cleanup.active_editor_matches_bridge_uri_after = false` must remain exact `false`, not be rewritten into a fallback `true` by jq boolean-default drift;
- the same live verifier must preserve the full bridge runtime payload plus machine-readable capability drift, because the next beta blocker is no longer lower bridge launch truth but higher `visible_surface` UX observability; `scripts/proof_vscode_compact_chat_visible_surface_runtime_boundary.sh` is the bounded proof that the installed bridge bundle already supports `visible_surface` while the currently loaded runtime still omits it from the live result on this machine;
- `scripts/proof_vscode_compact_chat_runtime_refresh_boundary.sh` now proves the next sharper boundary for that same contour: public refresh candidates `command:workbench.action.restartExtensionHost` and `command:workbench.action.reloadWindow` both return through `code --open-url`, but on this machine they still do not make the loaded bridge runtime pick up `visible_surface`; after each refresh probe the bounded live verifier remains green on lower launch truth and still records `runtime_capability_drift.visible_surface_missing_from_runtime_result = true`;
- compact-chat host launch is now a fail-closed state machine: no launch request emits `available_not_requested`, missing launch command emits `bridge_unavailable`, default policy gate emits `disabled_by_policy`, opt-in command success emits `requested`, and opt-in command failure emits `launch_failed`; API notice-kind mapping must preserve those states instead of collapsing policy-disabled launches into a generic requested notice;
- `requested` / `launch_failed` are host command-contract proof states only; VS Code `vscode_code_chat_cli` stays command-contract proof, while `vscode_uri_amai_bridge` is now promoted only after workspace-local live verification artifact exists and otherwise fail-closes back to CLI/manual fallback. Even after that promotion, it is still not a full seamless UX proof without higher-level visible-surface / startup-restore outcome evidence;
- Hermes proof may validate sticky project profile, compact `.hermes.md`, MCP config and reconnect helpers, but that is not live Hermes agent behavior proof;
- remote onboarding proof must remain offline-deterministic: `proof_remote_onboarding.sh` validates remote SSH config plus sync flow through a fake-ssh harness, while `proof_remote_repo_sync_payload.sh` validates payload boundaries;
- documentation must keep `AMAI-AUDIT-CLIENT-001..004` separate from any future full seamless-host claim.

Local stack autostart boundary:
- `proof_stack_autostart.sh` proves deterministic `amai-stack.service` unit rendering only;
- `proof_bootstrap_volume_dirs.sh` proves `run_stack_service.sh` / `bootstrap_stack.sh` prepares required volume and rendered config paths before compose startup;
- `proof_onboarding.sh` may prove the real onboarding path printed and activated the user service on a host with working `systemd --user`;
- these proofs do not prove unattended headless boot after reboot without user login;
- docs must keep `AMAI-AUDIT-AUTOSTART-001` separate from any future linger/system-service reboot guarantee;
- broad reboot claims require an explicit `loginctl enable-linger` opt-in or system-level service mode plus a dedicated proof.

Fresh refresh 2026-04-25:
- `./scripts/proof_stack_autostart.sh` passed for deterministic service rendering.
- `./scripts/proof_bootstrap_volume_dirs.sh` passed for pre-compose volume/config path preparation.
- `./scripts/proof_onboarding.sh` passed on the live host and confirmed the install output includes `Amai stack autostart ready: amai-stack.service`.
- The accepted claim remains `systemd --user` user-manager autostart. A future claim about boot without login must add a separate linger/system-service proof and cannot reuse these results as reboot evidence.

Compatibility memory bridge boundary:
- `proof_memory_bridge_search.sh` is the required proof for `memory search` compatibility output;
- it must rebuild `target/release/amai` and `target/release/memory` before checking output, otherwise stale release binaries can hide source changes;
- `memory search` must always print `Почему вошло` and `Почему часть не вошла`, including zero-hit, cache-hit and missing-decision-trace paths;
- targeted `cargo test --bin memory` should cover explanation fallbacks when bridge output formatting changes;
- docs must keep `AMAI-AUDIT-BRIDGE-001` tied to release bridge behavior, not to a `cargo run` developer path.

Fresh refresh 2026-04-25:
- `./scripts/proof_memory_bridge_search.sh` passed after rebuilding release binaries.
- `cargo test --quiet --bin memory` passed with 11 tests.
- This proves the compatibility bridge output/explainability contract only; retrieval relevance quality remains governed by context-pack/retrieval proofs.

Operational MCP launcher freshness boundary:
- `proof_mcp_launcher_freshness.sh` is the required proof for MCP stdio launcher changes;
- when `cargo` is available, `scripts/run_mcp_stdio.sh` and `scripts/run_mcp_stdio.ps1` must prefer `cargo run --release --quiet -- mcp serve` over `target/release/amai`;
- `target/release/amai` is allowed only as a no-cargo degrade path, not as the default onboarding path;
- shell startup diagnostics must not write to stdout before JSON-RPC, because stdout is the MCP protocol stream;
- docs must keep `AMAI-AUDIT-MCP-LAUNCHER-001` separate from any future cross-client live-host behavior claim.

Fresh refresh 2026-04-25:
- `./scripts/proof_mcp_launcher_freshness.sh` passed.
- It verified JSON-RPC initialize and repeated `amai_continuity_startup` through `scripts/run_mcp_stdio.sh` while a cargo shim proved the selected command was `cargo run --release --quiet -- mcp serve`.
- This proof does not validate every IDE/client host; it validates the shared stdio launcher path and the protocol-clean startup boundary.

MCP handshake/runtime contract boundary:
- `proof_mcp.sh` is the required proof for MCP tool/prompt/manifest/startup contract changes;
- `prompts/list` must expose exactly `amai-onboarding`, `amai-continuity-startup` and `amai-context-pack`;
- runtime startup artifact references must stay synchronized across `src/mcp.rs`, `.amai/onboarding/project-chat-startup-contract.json`, `.amai/onboarding/project-chat-startup-agent-contract.json`, managed startup instructions and docs;
- current runtime artifact version is `workspace-startup-runtime-state-v4`; stale `v3` wording is a documentation defect;
- startup must not require every registered target project to carry its own `config/token_budget_profiles.toml`; if the target project lacks that file, token-budget accounting must fall back to Amai's own config so `amai_continuity_startup` stays usable for normal external projects;
- MCP proof sessions must bind the spawned MCP server to an explicit generated `CODEX_THREAD_ID` and reuse that same id for `amai_observe_whole_cycle_turn`, otherwise proof token events can inherit an unrelated live-thread scope and fail the observed-scope contract;
- docs must keep `AMAI-AUDIT-MCP-CONTRACT-001` and `AMAI-AUDIT-MCP-CONTRACT-002` separate from launcher freshness and from live client behavior claims.

Fresh refresh 2026-04-25:
- `./scripts/proof_mcp.sh` passed with `proof_scope=full`, `critical=0`, `unknown=0`, `memory_matrix_tasks_failed=0`.
- The proof observed prompts `amai-context-pack`, `amai-continuity-startup`, `amai-onboarding`, 14 MCP tools and `83.69098712446352%` token savings.
- This is accepted as MCP handshake/runtime/proof-session evidence only; client UX and host launcher behavior stay in their own proof lanes.

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

## Probabilistic / statistical reinforcement changes

Это не отдельный stage-close shortcut.
Любой будущий Bayesian / calibration / drift / Markov / Poisson / regression contour обязан проходить поверх уже существующих gate laws.

Если работа идёт по [AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md](AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md), implementer обязан:
- использовать `canonical execution order` из этого документа как обязательную очередь;
- не перепрыгивать через `Queue 0-5` без явного doc update;
- не менять exact `v1` methods/contracts/filenames молча;
- не расширять production scope beyond того, что этот execution-spec явно разрешает.

### Что обязательно проверить

- probabilistic surface не подменяет current truth-source;
- новая confidence-модель не переписывает verified truth только потому, что у неё красивое число;
- benchmark improvement не объявляется без явного baseline/candidate сравнения;
- significance и drift проверяются на raw-run данных, а не на красивом summary;
- lifecycle/forgetting math остаётся explainable и audit-safe;
- regression используется только как explain/forecast surface, а не как authoritative routing/truth layer;
- KAN-style context-pack utility explain остаётся read-only shadow projection
  поверх already allowed candidates, а не ranking/routing/truth layer;
- capacity/arrival model не превращается в runtime enforcement truth до observed validation.

### Какой proof path обязателен

- baseline vs candidate raw-result lane;
- measured calibration / benchmark honesty surface;
- drift report по score/latency/verdict distributions;
- explain trace для lifecycle transitions и forgetting decisions;
- companion non-regression bundle по затронутым соседним осям (`speed / accuracy / quality / truth`).

Если contour конкретно про `KAN-style context-pack utility explain` из
`Candidate Queue 4A`, дополнительно обязательны:
- этот gate additive and narrower than the existing truth/policy/retrieval
  gates; он не отменяет и не ослабляет обычные gates для schema, policy,
  routing, retrieval, scope, provenance или truth-sensitive changes;
- все поля ниже являются target contract for a not-yet-materialized surface,
  not evidence that the surface already exists;
- any future implementation that consumes these fields must mark the path as
  speculative/contract-based until `shadow_approved` exists and must not let the
  fields affect critical truth, policy, retrieval or write paths;
- future code must include an explicit compile-time or runtime guard that blocks
  serializers, logging helpers, dashboard helpers and other consumers from using
  these fields in live decision paths before `shadow_approved`;
- state machine:
  - `spec_only`;
  - `trace_only`;
  - `offline_replay`;
  - `shadow_observe`;
  - `dashboard_internal`;
  - `user_visible_opt_in`;
- no-authority flags в каждом payload:
  - `truth_authority = false`;
  - `routing_authority = false`;
  - `ranking_authority = false`;
  - `runtime_authority = false`;
  - `forgetting_authority = false`;
  - `promotion_authority = false`;
- allowed-candidate contract:
  - only candidates already admitted by project/namespace/scope filters;
  - no semantic/Qdrant-only scope expansion;
  - lexical/symbol-first law remains binding;
  - enabled-vs-disabled context-pack output and candidate order stay byte/JSON
    equivalent until a separate adoption approval exists;
- raw trace contract:
  - `context_pack_id`;
  - `correlation_id`;
  - `decision_trace_id`;
  - `project_code`;
  - `namespace_code`;
  - `retrieval_mode`;
  - `allowed_candidate_scope`;
  - `candidate_summary`;
  - `rerank_summary`;
  - `evidence_sufficiency`;
  - `final_decision`;
  - `model_visible_context_pack_payload_sha256`;
  - `candidate_order_sha256`;
  - `feature_schema_version`;
- output status contract:
  - accepted states are only `measured`, `insufficient_sample`, `ood`,
    `not_materialized` and `unknown`;
  - missing raw trace, stale schema, one-sided labels, OOD, Qdrant dependency
    failure, scope mismatch or raw/dashboard SHA drift must fail closed instead
    of producing best-guess explanations;
- dashboard/raw parity:
  - raw snapshot payload is source of truth for the projection;
  - dashboard card may only render raw/observe fields;
  - no dashboard-only computed truth;
  - raw payload SHA and dashboard payload SHA must be surfaced;
- baseline/challenger proof:
  - compare current explain summary, simple transparent baseline and KAN-style
    candidate on held-out traces;
  - include feature-family ablation;
  - include perturbation/stability checks for top contributors;
  - include CI/sample-size/drift metadata before any `measured` claim.

Proof bundle for `KAN-style context-pack utility explain`:
- targeted Rust contract tests for trace schema, feature extraction, status
  states and authority flags;
- enabled-vs-disabled context-pack equality test;
- adversarial empty/ambiguous trace tests;
- cross-project and namespace leak guard;
- Qdrant unavailable degradation test;
- redaction/secret hygiene check;
- baseline-vs-candidate raw-result lane with CI/sample-size/drift;
- `./scripts/proof_memory_task_matrix.sh`;
- `./scripts/proof_mcp_task_matrix.sh`;
- `./scripts/proof_observability.sh`;
- `cargo run -- observe snapshot`;
- `cargo run -- observe sla-check`.

If that contour touches retrieval/vector/context-pack runtime, add:
- `./scripts/proof_accuracy.sh`;
- `./scripts/proof_performance.sh`;
- `./scripts/proof_load.sh`;
- cold benchmark bundle;
- external/Qdrant bundle.

Passing this gate means `evidence-only internal shadow explanation is safe to
surface as read-only projection`.
It does not mean KAN is adopted as core ranking, routing, truth, lifecycle or
promotion authority.

Если contour конкретно про `Markov / hazard lifecycle`, дополнительно обязательны:
- canonical state-model contract:
  - какие exact lifecycle-состояния считаются наблюдаемыми;
  - из каких уже существующих полей они вычисляются;
- transition dataset contract:
  - какие таблицы/трейсы считаются источником переходов;
  - как считается dwell-time;
  - как разделяются cohorts по `retention_class / decay_policy / derivation_kind / freshness/utility/access bands`;
- operator-facing explain surface:
  - expected next state;
  - transition reason;
  - cohort risk summary;
  - expected residency / urgency;
- measured validation, что recommendation contour реально улучшает forgetting/revalidation discipline, а не просто строит красивую матрицу.

### Что запрещено

- выпускать Bayesian/Markov/Poisson contour только по теоретической красоте;
- выдавать regression/ranking score за truth verdict;
- поднимать новую confidence-проекцию выше governed policy/evidence path;
- объявлять significance/drift достаточной заменой product proof.

Для `Markov / hazard lifecycle` отдельно запрещено:
- строить одну усреднённую transition matrix на весь memory contour без cohort separation;
- давать модели право напрямую запускать destructive prune/archive action;
- использовать probabilistic lifecycle score как замену `truth_state / verification_state / policy decision`;
- выводить lifecycle-рекомендации без explain trace и before/after audit trail.

## Честное правило про evidence gap

Если у этапа ещё нет dedicated proof harness, агент не имеет права закрывать этап “по ощущениям”.

Тогда он обязан:
- явно назвать evidence gap;
- не ставить checkbox;
- добавить создание proof/benchmark в обязательную часть этого этапа.
