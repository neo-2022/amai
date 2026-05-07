# Memory Bench Runbook

This runbook prepares Amai for external memory benchmarks without introducing Python-only core paths.

Status-truth boundary:
- `cargo run -- benchmark external-check` is a source/tool preflight only; unstable upstream probes may be marked blocked/timeout and must not be interpreted as Amai runtime/evaluator maturity.
- `./scripts/proof_memory_external_benchmarks.sh` proves dataset/download readiness, adapter workspace creation, normalized Amai case preparation, and one synthetic `external-memory-run` + `external-memory-score` smoke for the CLI runtime/scoring contract.
- `./scripts/proof_memory_external_real_bounded.sh` proves bounded, dataset-specific execution evidence for LongMemEval only: real normalized `longmemeval_s_cleaned` requests, real Amai runtime predictions, runtime metrics, and baseline score output for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT` cases.
- `./scripts/proof_memory_external_real_bounded_ama_bench.sh` proves bounded, dataset-specific execution evidence for AMA-Bench only: real normalized `ama_bench_manual` requests, real Amai runtime predictions, runtime metrics, and baseline score output for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT` cases while keeping official scorer contract unavailable/blocker-visible.
- `./scripts/proof_memory_external_real_bounded_memoryagentbench.sh` proves bounded, dataset-specific execution evidence for MemoryAgentBench only: real normalized `memoryagentbench_conflict_resolution`, `memoryagentbench_long_range_understanding`, and `memoryagentbench_test_time_learning` requests, real Amai runtime predictions, runtime metrics, answer-source/relevance accounting, baseline score output, and explicit boolean-typed `runtime_corpus_sha256` / `runtime_corpus_reused_from_previous_case` accounting for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT` cases while keeping official scorer contract unavailable/blocker-visible.
- `./scripts/proof_memory_external_real_bounded_memoryagentbench_accurate_retrieval_blocked.sh` proves a different bounded outcome for `memoryagentbench_accurate_retrieval`: on the current default bounded limit (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`), the slice now runs end-to-end with perfect bounded baseline answers plus proxy evidence of retrieval participation across all bounded cases, lexical/structural proxy support for the benchmark answers, no benchmark-specific shaping in the bounded run (`query_override_cases=0`, `window_override_cases=0`, `answer_extraction_cases=0`, `generic_runtime_maturity=true`), answer-supporting top-ranked retrieval proxy across the whole bounded slice (`top_ranked_gold_answer_supported_retrieval_cases=3/3`), and top-ranked `anchored_fact_shape_proxy` support across the same fixed slice (`top_ranked_structural_fact_supported_cases=3/3`). The runtime also now proves identical-corpus reuse inside that default bounded slice: one corpus hash across all three cases, `runtime_corpus_reused_cases=2`, and `index_project_ms=0` on the reused cases. A fresh profiling-guided runtime fix then corrected the synthetic-runtime edge-cache skip gate to match the actual `.md` runtime corpus shape instead of a stale `source_kind` assumption; in the current bounded run this drops cold first-case `index_project_ms` to about `0.87s`, bounded `index_project_ms.avg` to about `0.29s`, and bounded `total_case_ms.avg` to about `0.91s` while preserving the same bounded proof contract. This reuse is intentionally narrow: same-process, same-run, byte-identical materialized corpus reuse only, with a fail-closed guard on the currently materialized `paths.txt`; it is not a persistent cross-run cache, not semantic equivalence, and not a claim about hidden environment identity outside the materialized runtime corpus. It still must stay blocker-visible instead of being promoted into the fully trusted bounded-runtime whitelist because semantic relevance maturity is still false, benchmark-grade scorer parity is still absent, and `latency_maturity=false` remains a bounded-only disclaimer rather than a cold-index-dominance blocker. The boundary accounting is carried by explicit runtime metric flags rather than question-text re-derivation. The proof output also writes `bounded-proof-contract.json`; use it as the machine-readable disclaimer for the bounded blocked scope, the non-semantic interpretation of retrieval-hit rates, the non-semantic interpretation of the structural-fact proxy, and the still-non-grade latency profile. The script now also re-runs targeted Rust negative tests for malformed resume identity fields and for the `paths.txt` same-hash reuse gate, so the reject-path story is not happy-path-only; those tests prove reject-path behavior, not a broader filesystem-stability guarantee.
- Skip-gate contract for this bounded runtime lane: edge-cache upserts may be skipped only when `index_project` runs with `skip_embeddings=true`, `preserve_namespace_documents=true`, and every collected runtime file in the fixed slice is an `.md` artifact. The gate is evaluated once from immutable launcher arguments plus the collected file list before the sequential per-file indexing loop; it does not change PostgreSQL writes, does not alter retrieval/query logic, and does not create a persistent cache claim. In this bounded lane `concurrency=1`, so no extra cross-task synchronization claim is being made.
- `./scripts/proof_memory_external_official_judge.sh` proves the Rust-only LongMemEval official judge execution/log lane stays fail-closed without an explicit live API gate or API key, embeds the upstream answer-check prompt templates, and records `external_memory_official_judge_execution_v1` without claiming scorer parity.
- `./scripts/proof_memory_external_official_judge_api_failure.sh` proves local live-API failure handling for the official judge lane: simulated rate-limit/upstream/transport/response-contract failures must write a blocked summary, must not materialize eval-results JSONL, and must not persist the configured key value in the summary.
- `./scripts/proof_memory_external_official_judge_live_bounded.sh` is the bounded operator live lane for `longmemeval_s_cleaned`: with the configured API key it runs the official judge and then `external-memory-official-score`; without the key it must stay green only by proving `official_judge_api_key_not_materialized` and no eval-results log.
- `./scripts/proof_memory_external_official_judge_live_balanced.sh` is the six-type bounded operator lane: it selects one raw `longmemeval_s_cleaned` record for each official LongMemEval question type, normalizes through `external-memory-prepare --source-path`, runs Amai predictions for those six cases, then delegates to the same official judge live guard.
- `./scripts/proof_memory_external_official_score_reconcile.sh` proves the Rust-only official LongMemEval eval-log reconciliation lane: it reads upstream-style `evaluate_qa.py` JSONL output, checks the `gpt-4o-2024-08-06` label contract, computes official-style overall/task-averaged/abstention metrics, and keeps live judge parity fail-closed.
- It does not prove full external benchmark-grade memory maturity.
- Full maturity requires real benchmark runtime predictions, real benchmark scoring across the materialized datasets, and upstream scorer parity where the upstream benchmark has its own scorer.
- Fresh rerun note 2026-04-25: the proof stayed green in this bounded lane. MemoryAgentBench prep can create multi-GiB `cases.jsonl`/`requests.jsonl` artifacts, so this proof must not be presented as a lightweight recurring full benchmark runtime.
- Fresh bounded real runtime note 2026-04-26: the bounded LongMemEval proof is limited to the named dataset and limit. It is real runtime+baseline-score evidence, not upstream scorer parity and not full dataset maturity. It also guards that runtime metrics show retrieval hits after relaxed benchmark-query retry instead of silently accepting fallback-only answer extraction.
- Fresh bounded MemoryAgentBench runtime note 2026-04-27: `./scripts/proof_memory_external_real_bounded_memoryagentbench.sh` now materializes three small MemoryAgentBench slices with real runtime predictions, baseline score output and runtime metrics boundaries: `memoryagentbench_conflict_resolution`, `memoryagentbench_long_range_understanding`, and `memoryagentbench_test_time_learning`. This remains `bounded_real_runtime_score` only: `memoryagentbench_overall_score` is baseline-only, retrieval relevance is still proxy/lexical, and official scorer parity remains unavailable. The verified profiles are operationally different: `conflict_resolution` showed some fallback-scan use, `long_range_understanding` ran with much larger contexts and higher case latency, and `test_time_learning` ran with very large context packs and high latency but does not imply general adaptive learning maturity beyond the bounded slice. Fresh reruns also materialize and bounded-proof-check the joint invariant `top_ranked_relevance_and_gold_answer_supported_retrieval_cases <= top_ranked_gold_answer_supported_retrieval_cases` for these named profiles; this is a bounded proof contract for those datasets, not a general promotion gate.
- Fresh bounded MemoryAgentBench corpus-identity note 2026-04-29: the same named-profile proof now fail-closes on boolean-typed `runtime_corpus_sha256` and `runtime_corpus_reused_from_previous_case`. On the current default bounded limit (`AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`), the bounded truth is explicitly split by profile instead of being flattened into one cache story: `memoryagentbench_conflict_resolution` and `memoryagentbench_test_time_learning` each materialize one identical runtime corpus across the fixed `3-case` slice with `runtime_corpus_reused_cases=2` and `index_project_ms=0` on reused cases, while `memoryagentbench_long_range_understanding` materializes three different runtime corpus hashes and therefore `runtime_corpus_reused_cases=0`. This is same-process, same-run runtime-corpus identity evidence only; it must not be documented as a persistent cache, hidden-environment equivalence, or general reproducibility maturity.
- Fresh bounded MemoryAgentBench `accurate_retrieval` note 2026-04-27: a bounded `3-case` slice now materializes runtime, perfect bounded baseline score output, retrieval-backed predictions for all bounded cases, retrieval-supported gold answers for all bounded cases, answer-supporting top-ranked retrieval for all bounded cases, and relevance-supporting top-ranked proxy retrieval for all bounded cases. In the current bounded run, `memoryagentbench_overall_score=1.0`, `exact_match=3/3`, `retrieval_answer_cases=3`, `fallback_scan_cases=0`, `relevant_retrieval_evidence_cases=3`, `top_ranked_relevant_retrieval_cases=3`, `gold_answer_supported_retrieval_cases=3`, `top_ranked_gold_answer_supported_retrieval_cases=3`, `top_ranked_relevance_and_gold_answer_supported_retrieval_cases=3`, and `benchmark_specific_shaping_boundary` is now clean with `query_override_cases=0`, `window_override_cases=0`, `answer_extraction_cases=0`, `benchmark_specific_shaping_present=false`, `generic_runtime_maturity=true`. Runtime resume is now fail-closed on artifact identity: `requests.jsonl` must carry non-empty `bench` and `dataset`, persisted `.case-metrics.jsonl` rows must carry the explicit shaping flags, and the newer top-ranked retrieval telemetry keys must also be present or the runtime refuses to reuse them. This means the slice is now stronger bounded generic-runtime evidence than before, but the contour is still not fully trusted benchmark-grade maturity because semantic relevance remains proxy/lexical and upstream scorer parity is still absent.
- Fresh answer-source accounting note 2026-04-26: runtime metrics now include `answer_source_boundary` with `retrieval_hit_cases`, `retrieval_answer_cases`, `fallback_scan_cases` and `semantic_precision_maturity=false`. This makes fallback-assisted predictions machine-visible and keeps semantic retrieval precision separate from baseline exact-match scoring.
- Fresh retrieval-relevance proxy note 2026-04-26: runtime metrics also include `retrieval_relevance_boundary` (`external_memory_retrieval_relevance_boundary_v1`) with query-overlap proxy accounting for retrieved snippets. This is a fail-closed relevance signal, not a gold-labeled semantic relevance judge.
- Fresh gold-answer support note 2026-04-26: runtime metrics also include `gold_answer_relevance_boundary` (`external_memory_gold_answer_relevance_boundary_v1`) with benchmark-answer lexical support accounting for retrieved snippets. This proves bounded gold-label data flow and answer-support accounting only; it is not upstream scorer parity and not semantic relevance maturity.
- Fresh official scorer contract note 2026-04-26: score output now includes `official_scorer_boundary` (`external_memory_official_scorer_boundary_v1`) for LongMemEval. It records the official scorer input/model/metric contract from the upstream `evaluate_qa.py` and `print_qa_metrics.py` scripts, but keeps `official_upstream_scorer_parity=false` until a live official judge run and upstream metrics log are materialized.
- Fresh official judge execution/log note 2026-04-26: `external-memory-official-judge` now materializes `external_memory_official_judge_execution_v1` and can write upstream-style eval-results JSONL only when `--allow-live`, the official `gpt-4o-2024-08-06` model and the configured API key env are all present. Its default/offline proof is fail-closed and does not create an eval-results log.
- Fresh official judge API-failure note 2026-04-26: `proof_memory_external_official_judge_api_failure.sh` uses local fake Chat Completions-compatible responses for HTTP 429/503, HTTP 200 malformed JSON, HTTP 200 empty/missing `choices[0].message.content`, plus a connection-refused transport path. These runs are not live-key evidence; they prove that non-key API and response-contract failures stay blocked, leave eval-results absent, and keep the dummy key value out of the summary even when a hostile fake error body echoes it.
- Fresh bounded live-operator note 2026-04-26: `proof_memory_external_official_judge_live_bounded.sh` wires the bounded real LongMemEval artifacts to the live official judge and reconciliation commands. The local no-key run is blocker-visible (`official_judge_api_key_not_materialized`) and deliberately leaves `official-live-eval-results.jsonl` absent.
- Fresh balanced live-operator note 2026-04-26: `proof_memory_external_official_judge_live_balanced.sh` now covers all six LongMemEval official question types in one bounded run. This avoids a key-backed bounded live run being blocked merely because the first-N sample did not include every official type; it still remains bounded and no-key fail-closed locally.
- Fresh official score reconciliation note 2026-04-26: `external-memory-official-score` now materializes `external_memory_official_score_reconciliation_v1` from an upstream-style eval-results JSONL log. This is log-contract and metric reconciliation only; it does not verify that the log came from a live official LLM judge run.
- Relaxed benchmark-query retry is a recall recovery mechanism only. It can prove that retrieval participated in a bounded run, but it does not prove semantic precision or answer relevance by itself; the query-overlap relevance proxy and baseline score evidence still stay separated from upstream scorer parity and full maturity.

Accepted evidence boundary:
- A non-empty `latest/cases.jsonl` plus a manifest with zero missing `question/context/id` proves normalized-case prep only.
- The manifest `prep_validation` boundary (`external_memory_prep_validation_v2`) must keep `written_case_count` aligned with written cases and fail closed on `no_cases_materialized`, missing required fields, duplicate `case_id` values, and invalid normalized field types or shapes for `bench`, `dataset`, `case_id`, `question`, `context`, `answer`, and `metadata`.
- A `total=0` manifest, empty `cases.jsonl`, manual marker file, or adapter workspace proves no normalized benchmark maturity.
- A synthetic exact-match smoke proves only that `external-memory-run` and `external-memory-score` can execute their command contract.
- A stale `*.status.json` with `stage = "running"` is failed/unfinished runtime evidence until rerun or reconciled.
- A benchmark-grade claim requires real dataset predictions, scored real outputs and upstream scorer parity where an upstream scorer exists.
- Bounded real predictions and baseline score can be cited only as bounded, dataset-specific runtime/scoring evidence for the named dataset and limit; they must stay separate from upstream parity and full-dataset maturity claims.
- If `chunk_hits + document_hits == 0` for the bounded real proof metrics, the proof must fail as `no_retrieval_evidence`; fallback-only answer extraction is not accepted as real retrieval/runtime evidence.
- `predictions.jsonl.metrics.json.answer_source_boundary.boundary_version` is currently `external_memory_answer_source_boundary_v1`; `retrieval_answer_cases > 0` is required for the bounded proof, but any `fallback_scan_cases > 0` remains evidence that full semantic retrieval precision is not proven.
- `predictions.jsonl.metrics.json.retrieval_relevance_boundary.boundary_version` is currently `external_memory_retrieval_relevance_boundary_v1`; `relevant_retrieval_evidence_cases > 0` is required for the bounded proof, `top_ranked_relevant_retrieval_cases` must never exceed `relevant_retrieval_evidence_cases`, and `judge_kind=query_overlap_proxy` keeps `semantic_precision_maturity=false`.
- For the bounded `memoryagentbench_accurate_retrieval` proof, `top_ranked_relevant_retrieval_rate == 1.0` is an exact fixed-slice contract for `AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`, not a probabilistic maturity threshold. The corresponding `bounded-proof-contract.json` must keep explicit proxy failure modes surfaced: lexical overlap can miss a semantically correct snippet, and topical overlap can pass a non-answer snippet.
- If `retrieval_relevance_boundary.boundary_version` or `judge_kind` changes, refresh the bounded proof contract wording and rerun the proof before reusing old maturity wording.
- `predictions.jsonl.metrics.json.gold_answer_relevance_boundary.boundary_version` is currently `external_memory_gold_answer_relevance_boundary_v1`; non-empty `gold_labeled_cases` plus the `gold_answer_supported_retrieval_cases <= retrieval_evidence_cases` invariant are required for the bounded proof, `top_ranked_relevance_and_gold_answer_supported_retrieval_cases` must never exceed `top_ranked_gold_answer_supported_retrieval_cases`, and `judge_kind=gold_answer_lexical_overlap` keeps `semantic_precision_maturity=false`. `gold_answer_supported_retrieval_cases = 0` is a valid blocker-visible result when retrieved evidence does not lexically contain the benchmark answer label; it must not be treated as proof failure or semantic maturity.
- `predictions.jsonl.metrics.json.structural_fact_relevance_boundary.boundary_version` is currently `external_memory_structural_fact_relevance_boundary_v1`; for bounded `accurate_retrieval`, `judge_kind=anchored_fact_shape_proxy`, `proxy_applicable_cases > 0`, and `top_ranked_structural_fact_supported_cases <= proxy_applicable_cases` are required. This is stronger than raw term overlap because it checks whether the top-ranked snippet structurally carries the asked fact pattern, but it is limited to those structural fact question shapes only. It must not be documented as generic retrieval accuracy, broader benchmark maturity, or semantic relevance proof.
- `top_ranked_relevance_and_gold_answer_supported_retrieval_cases` is defined narrowly and fail-closed: count a case only when the top-ranked retrieved snippet both satisfies the bounded relevance proxy (`retrieval_top_ranked_score >= relevance_threshold_score`) and lexically supports the benchmark gold answer label (`retrieval_gold_answer_top_supported=true`). If top-ranked gold-answer support exists without top-ranked relevance proxy support, the boundary must surface blocker `top_ranked_gold_answer_support_without_relevance_proxy` rather than silently treating the case as jointly supported.
- The negative-path regression for that blocker is Rust-native: `memory_runtime_gold_answer_relevance_boundary_requires_proxy_on_top_ranked_gold_support`. If the blocker semantics or field shape changes, rerun that test and the bounded proof before reusing old wording.
- `retrieval_payload_top_ranked_preview` is an explainability surface, not a new relevance judge. For bounded `accurate_retrieval`, if `retrieval_payload_top_ranked_gold_answer_supported=true`, the preview should be centered on the answer-bearing span rather than blindly taking the first 240 characters. This keeps the human-visible winner preview aligned with the already-materialized support verdict without upgrading semantic maturity.
- `external-memory-score.evidence_boundary.boundary_version` is currently `external_memory_score_evidence_boundary_v1`; any semantic change to the bounded score boundary requires updating the proof and this runbook together.
- `external-memory-score.evidence_boundary.maturity_blocking_reasons` is expected to include `baseline_scorer_only`, `official_upstream_scorer_not_integrated` and `full_dataset_runtime_not_proven_by_this_score` until a separate full-runtime/upstream-parity scorer lane exists.
- `external-memory-score.official_scorer_boundary.boundary_version` is currently `external_memory_official_scorer_boundary_v1`; for LongMemEval it must expose `source_kind=official_longmemeval_llm_judge_contract`, `metric_model=gpt-4o-2024-08-06`, `local_contract_materialized=true`, `requires_live_llm_judge=true` and `official_upstream_scorer_parity=false`.
- `external-memory-score.official_scorer_boundary.official_prompt_templates_embedded` must stay `false` in this contract-only lane. Prompt-template parity is not claimed until the live official scorer lane materializes the upstream prompt execution/log boundary.
- `external-memory-score.official_scorer_boundary.maturity_blocking_reasons` must include `live_official_llm_judge_not_run`, `official_eval_log_not_materialized`, `official_upstream_metrics_not_materialized` and `official_prompt_templates_not_embedded` until the official scorer is actually run and reconciled.
- `external-memory-official-judge.boundary_version` is currently `external_memory_official_judge_execution_v1`; it embeds the LongMemEval upstream answer-check prompt templates, refuses non-official metric models, and writes eval-results JSONL only after a live API-gated run. It must keep `official_upstream_scorer_parity=false` because metric reconciliation and full dataset runtime evidence are separate gates.
- `proof_memory_external_official_judge_api_failure.sh` may be cited only as local API/response-contract failure evidence. It does not prove a real provider outage, a real rate-limit window, a real authorized-key run, or upstream scorer parity.
- `proof_memory_external_official_judge_live_bounded.sh` may be cited only as bounded live-operator evidence. A no-key green run proves fail-closed secret gating, not live judge maturity; a key-backed run proves only the bounded live log plus its current reconciliation boundary and still does not prove full dataset maturity or upstream parity.
- No-key/offline official judge artifacts, including the default proof plus bounded and balanced live-operator summaries, must not contain `[REDACTED_OFFICIAL_JUDGE_API_KEY]`: that marker is valid only when a materialized test/live key value was actually present in provider output and was removed before persistence.
- This hygiene check is scoped to no-key/offline artifact integrity only; it is not evidence of live judge success, upstream scorer parity, or benchmark-grade maturity.
- On key-backed runs, `proof_memory_external_official_judge_live_bounded.sh` must invoke the Rust `external-memory-secret-scan` verifier across every regular file in the proof output directory, including the produced summary, eval-results and reconcile artifacts. Any configured API key value match is a hard proof failure; only the env var name may appear in artifacts.
- `proof_memory_external_official_judge_live_balanced.sh` may be cited only as six-type bounded live-operator evidence. It proves official question-type coverage for the bounded sample and source-path normalization, not representative dataset distribution or full LongMemEval maturity.
- `external-memory-official-score.boundary_version` is currently `external_memory_official_score_reconciliation_v1`; a reconciled output requires all six LongMemEval question types, `autoeval_label.model = gpt-4o-2024-08-06`, matching `question_id` values, and no invalid/missing eval-log records.
- `external-memory-official-score.official_upstream_scorer_parity` must remain `false` until live official judge provenance and a full dataset runtime+score lane are proven. A valid eval-results JSONL file proves only that the official log contract can be consumed and summarized.

Proxy relevance limitations:
- The query-overlap proxy is expected to catch only direct lexical overlap between the benchmark question and retrieved snippets; it is intentionally conservative evidence that retrieval participated, not proof that the snippet semantically answers the question.
- Known weak cases include paraphrased questions, abstract or multi-hop reasoning, domain-shifted wording, negation, and retrieved snippets that repeat query terms while missing the answer.
- The proxy is fail-closed: a case is never counted as relevant without non-empty retrieval evidence, and all maturity fields remain blocked until a gold-labeled semantic relevance judge is integrated.
- The gold-answer overlap boundary is also lexical: it uses the normalized benchmark `answer` field as a label and checks whether retrieved snippets contain that answer. This catches answer-bearing retrieval evidence, but it misses paraphrased answers and can still pass snippets that mention the answer without proving reasoning quality.
- The gold-answer boundary is fail-closed: supported cases cannot exceed non-empty retrieval evidence cases, and all maturity fields remain blocked until an official upstream scorer or gold-labeled semantic judge is wired in.

Consensus records:
- `AMAI-AUDIT-EXTMEM-001`: Stage 0-10 internal closure is not external memory benchmark maturity; keep this as `partial` until real runtime+score and upstream parity exist.
- `AMAI-AUDIT-EXTMEM-002`: LoCoMo `qa[]` expansion and session rendering are verified for normalized-case prep; do not promote that to full benchmark maturity.
- `AMAI-AUDIT-EXTMEM-003`: AMA-Bench has bounded real runtime+baseline-score evidence from the manual HF dataset install, but full dataset runtime, benchmark-grade maturity and any official scorer parity remain open.
- `AMAI-AUDIT-EXTMEM-004`: `external-memory-run/score` are currently command-contract smoke surfaces; full evaluator maturity requires real benchmark runs.
- `AMAI-AUDIT-EXTMEM-005`: bounded, dataset-specific LongMemEval runtime+baseline-score proof is materialized for a small limit, with relaxed benchmark-query retry, answer-source accounting, query-overlap retrieval relevance, benchmark-answer lexical support, official scorer contract boundary and official eval-log reconciliation guarded by metrics/proofs; live upstream scorer parity, gold-labeled semantic retrieval precision and full benchmark maturity remain open.

Current claim inventory:
- code surface: `src/external_benchmark.rs`, `config/external_benchmark_targets.toml`, `config/external_benchmark_datasets.toml`;
- proof surface: `./scripts/proof_memory_external_benchmarks.sh`;
- bounded real proof surface: `./scripts/proof_memory_external_real_bounded.sh`;
- bounded AMA-Bench real proof surface: `./scripts/proof_memory_external_real_bounded_ama_bench.sh`;
- bounded MemoryAgentBench real proof surface: `./scripts/proof_memory_external_real_bounded_memoryagentbench.sh`;
- official judge execution/log proof surface: `./scripts/proof_memory_external_official_judge.sh`;
- bounded official judge live-operator proof surface: `./scripts/proof_memory_external_official_judge_live_bounded.sh`;
- balanced six-type official judge live-operator proof surface: `./scripts/proof_memory_external_official_judge_live_balanced.sh`;
- official score reconciliation proof surface: `./scripts/proof_memory_external_official_score_reconcile.sh`;
- raw lane: `state/external-benchmarks/memory/**/latest/{cases,requests,manifest}.json*`, synthetic predictions/score output, and bounded real outputs under `tmp/external-memory-real-bounded/`;
- accepted freshness: 2026-04-25 bounded prep rerun plus 2026-04-26 bounded real LongMemEval runtime+baseline-score proof;
- downgrade rule: any wording that implies benchmark-grade long-term memory maturity before real predictions, real scoring and upstream scorer parity must be marked `external-maturity-gap` or `proof-refresh-required`.

## Benchmarks

- LongMemEval
- AMA-Bench
- MemoryAgentBench
- LoCoMo

## Prepare datasets and adapter workspaces

```bash
./scripts/proof_memory_external_benchmarks.sh
```

Notes:
- AMA-Bench still uses a manual dataset install path. Place the downloaded HF file at:
  `state/external-benchmarks/datasets/ama-bench.manual`
- The current proof now materializes real normalized AMA-Bench cases from that file through the shared Rust prep contour.
- AMA-Bench now also has a bounded real runtime+baseline-score proof on top of prep, but full dataset runtime and benchmark-grade maturity remain separate open work.
- All other datasets are fetched via the external benchmark dataset catalog.

## Generate normalized cases for Amai

The proof script writes normalized JSONL cases into:

The current guarded normalized lanes are:
- LongMemEval: `longmemeval_s_cleaned`
- MemoryAgentBench: `accurate_retrieval`, `conflict_resolution`, `long_range_understanding`, `test_time_learning`
- LoCoMo: `locomo10`

AMA-Bench is now included in the normalized-case proof when the manual HF dataset file is present.
If `state/external-benchmarks/memory/ama_bench/ama_bench_manual/latest/` exists with empty files or `total=0`, treat it as stale placeholder evidence and rerun the proof.
MemoryAgentBench normalized prep may expand source parquet rows into very large context/request JSONL artifacts. Treat disk size and runtime as operational inputs for the future real benchmark lane, not as maturity evidence by themselves.

For a bounded real LongMemEval runtime+baseline-score proof:

```bash
./scripts/proof_memory_external_real_bounded.sh
```

This proof defaults to `AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`. It verifies real prepared requests, real Amai predictions, completed runtime status, case metrics, retrieval hits through relaxed benchmark-query retry, query-overlap retrieval relevance proxy metrics, and baseline score output. It deliberately does not claim official upstream scorer parity. Relaxed retry and query-overlap relevance are accepted only as bounded retrieval evidence; they are not gold-labeled semantic precision verdicts.

For a bounded real AMA-Bench runtime+baseline-score proof:

```bash
./scripts/proof_memory_external_real_bounded_ama_bench.sh
```

This proof also defaults to `AMAI_EXTERNAL_MEMORY_REAL_LIMIT=3`. It verifies bounded AMA-Bench prep, real runtime predictions, completed status, retrieval/accounting metrics and baseline score output, while keeping `official_scorer_boundary.source_kind=official_scorer_contract_unavailable`.

```
state/external-benchmarks/memory/<bench>/<dataset>/latest/cases.jsonl
state/external-benchmarks/memory/<bench>/<dataset>/latest/manifest.json
state/external-benchmarks/memory/<bench>/<dataset>/latest/requests.jsonl
```

Each JSONL line:

```json
{
  "bench": "longmemeval",
  "dataset": "longmemeval_s_cleaned",
  "case_id": "case-0001",
  "question": "...",
  "context": "...",
  "answer": "...",
  "metadata": { "...": "..." }
}
```

Requests file format (for model runtime):

```json
{
  "case_id": "case-0001",
  "prompt": "You are Amai... Answer:",
  "context": "...",
  "question": "..."
}
```

## Run Amai evaluation

This repo has an Amai runtime command for prepared requests. The proof script only exercises this command on a synthetic single-case smoke; real benchmark runs must still be executed per benchmark/dataset before claiming external benchmark maturity:

```bash
cargo run -- benchmark external-memory-run \
  --requests state/external-benchmarks/memory/<bench>/<dataset>/latest/requests.jsonl \
  --predictions state/external-benchmarks/memory/<bench>/<dataset>/latest/predictions.jsonl \
  --project amai \
  --namespace bench_runtime_<name>
```

If predictions are produced by another runner, store them as JSONL:

```json
{
  "case_id": "case-0001",
  "predicted_answer": "..."
}
```

Score with:

```bash
cargo run -- benchmark external-memory-score --cases <cases.jsonl> --predictions <predictions.jsonl> --output <score.json>
```

Scoring is a baseline exact/contains/abstention heuristic until official upstream scorers are added.
The proof script verifies this scorer with a synthetic exact-match case only; it is command-contract evidence, not upstream benchmark parity.
Stale `.status.json` files with `stage: "running"` are not closure evidence.

Reconcile an upstream-style LongMemEval official eval-results log with:

```bash
cargo run -- benchmark external-memory-official-score --cases <cases.jsonl> --eval-results <hypotheses.jsonl.eval-results-gpt-4o> --output <official-score.json>
```

This command consumes the official scorer output contract only. It computes overall, task-averaged and abstention metrics from `autoeval_label`, but keeps `official_upstream_scorer_parity=false` because live official judge provenance is outside the reconciler.
