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

## Текущий общий статус

### Общая оценка

Проект находится в состоянии:
- сильный current-state baseline уже materialized;
- target-state memory fabric уже хорошо спроектирован;
- кодовая реализация target-state уже начата по этапам;
- `Этапы 1-9` уже materialized и закрыты по текущему статусному чеклисту;
- текущий обязательный implementation focus смещён на `Этап 10. Governance, safety, evaluator loop`.

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

- [x] [Этап 0. Зафиксировать новую общую модель memory fabric](AMAI_GLOBAL_MEMORY_ROADMAP.md#L596)
- [x] [Этап 1. Scope и identity control plane](AMAI_GLOBAL_MEMORY_ROADMAP.md#L615)
- [x] [Этап 2. Typed memory envelope + provenance](AMAI_GLOBAL_MEMORY_ROADMAP.md#L840)
- [x] [Этап 3. Commitment / task graph](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1043)
- [x] [Этап 3A. Ранний procedural seed contour](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1111)
- [x] [Этап 4. Workspace restore pack](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1232)
- [x] [Этап 5. Semantic + temporal memory strengthening](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1269)
- [x] [Этап 6. Multi-agent shared/private memory](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1297)
- [x] [Этап 7. Compare + benchmark plane](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1320)
- [x] [Этап 8. Procedural memory](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1352)
- [x] [Этап 9. Forgetting, consolidation, pruning](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1521)
- [x] [Этап 10. Governance, safety, evaluator loop](AMAI_GLOBAL_MEMORY_ROADMAP.md#L1586)

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
- новый чат поднимает не только headline, а полезный рабочий пакет;
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
- этап ещё не закрыт;
- но temporal factual recall уже усилен в важном runtime contour:
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
- этап нельзя закрыть без passing отдельного procedural benchmark/proof.

Временный минимум:
- `./scripts/proof_procedural_seed.sh`
- procedural benchmark из compare-plane после materialization;
- evaluator/trust verification;
- shadow -> trial -> verified trace review.

### Этап 9. Forgetting, consolidation, pruning

Использовать:
- `./scripts/proof_forgetting_consolidation.sh`
- `./scripts/proof_observability.sh`

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
  - [stage gate](AMAI_GLOBAL_MEMORY_ROADMAP.md#L565);
  - [migration и kill-switch plan](AMAI_GLOBAL_MEMORY_ROADMAP.md#L577);
- если этот файл обновлялся как часть значимого этапа, status snapshot нельзя считать честно обновлённым без passing:
  - `./scripts/implementation_status_sync_guard.sh --json`;
- если этап значимый, checkbox нельзя ставить без passing:
  - `./scripts/maintainability_stage_close_guard.sh --json`;
- после этого здесь меняется checkbox;
- если этап только начат, но не закрыт, checkbox не ставится.

### Что ещё не materialized в коде

Пока ещё не materialized полностью:
- полностью бесшовный host-side переход длинной рабочей линии в новый чат во всех клиентах и средах.
- при этом prompt-side часть этого gap уже усилена:
  - `chat_start_restore` теперь поднимает не только headline/step;
  - новый чат уже видит `pending_return`, `ExecCtl return contract` и `required_task_set` явно и компактно.
  - canonical compact-chat path теперь уже сам запрашивает clean chat surface, если host launch bridge доступен.
  - startup/onboarding front-door снова materialized fail-closed:
    - `.amai/onboarding/project-chat-startup-contract.json` снова materialized на canonical пути;
    - compact-chat теперь явно различает `requested`, `bridge_unavailable` и `launch_failed`, а не теряет эти состояния внутри одного operator note.
  - operator/API surface тоже подтянут:
    - compact-chat notice больше не притворяется generic success, если launch bridge недоступен;
    - unavailable/not-requested/failed path теперь surfaced как отдельная truth-линия, а не только как внутренний JSON статус.
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

### Что сейчас в работе

Сейчас активный implementation focus:
- завершение всех 10 этапов memory fabric;
- удержание Stage 1-10 bundle в зелёном non-regression состоянии.
- scientific reinforcement overlay после закрытия Stage 1-10:
  - [AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md](AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md);
  - это уже не просто synthesis-note, а authoritative execution-spec для advisory/proof-grade probabilistic/statistical contours;
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

Все 10 этапов закрыты:
- Этап 0-9: ✅ закрыты ранее;
- Этап 10: ✅ закрыт (governance/safety/evaluator loop materialized).

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
- governance/safety/evaluator loop уже закрыт;

После закрытия Stage 1-10 следующий честный надстроечный contour такой:
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
- `confidence/calibration`
  - `concept-only`
  - out-of-scope для текущего production overlay; отдельный measured approval нужен до реализации.
- `benchmark significance + drift`
  - `planned`
  - это первый обязательный implementation queue после Queue 0 baseline freeze.
- `Markov/hazard lifecycle`
  - `concept-only`
  - execution path уже определён, но до Queue 2/3 materialization contour ещё не implemented.
- `Poisson capacity`
  - `blocked by proof/data`
  - execution path определён как Queue 5 forecast-only contour.
- `regression explain surface`
  - `planned`
  - execution path определён как Queue 4 read-only explain contour.

### Фундаментальные blocker-ы

На текущий момент фундаментальных blocker-ов к старту scientific execution overlay не зафиксировано.

Есть только нормальные дисциплинарные риски:
- drift между current-state docs и target-state docs;
- попытка перепрыгнуть через очереди `Queue 0-5`;
- попытка кодить без stage gate;
- попытка принять compare-plane или procedural seed за уже завершённый full procedural memory contour;
- попытка трактовать `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` как обзор идей, а не как executable playbook.

### Главная честная незакрытая проблема

Самый важный product gap сейчас такой:
- длинные чаты с несколькими линиями работы ещё не переходят в новый чат полностью бесшовно во всех host/client средах.
- machine-readable и prompt-side restore для multi-line obligations уже materialized;
- compact-chat default/operator path теперь уже сам запрашивает clean chat surface и убирает лишний ручной шаг, если launch bridge доступен;
- onboarding/startup artifact path снова согласован с этим contract-ом и не теряет machine-readable startup source-of-truth после reinstall/proof path;
- compact-chat host-launch contour теперь truthfully surface-ит не только success, но и `bridge_unavailable` / `launch_failed`;
- compact-chat notice/API теперь честно объясняет `bridge_unavailable` и `available_not_requested`, а не выдаёт их за общий success-case;
- compact-chat теперь ещё и materialize-ит current client surface для non-VSCode fallback, чтобы manual path был клиент-специфичным, а не generic;
- compact-chat client surface теперь включает и concrete reconnect/open-new-chat assist commands, а не только путь до startup surface;
- compact-chat per-client assist теперь surfaced ещё и прямо в dashboard KPI selector, а не остаётся только в API/operator notice;
- remaining gap теперь сузился до сред, где host/client не даёт Amai честно открыть clean chat surface автоматически.

Это не должно забываться.
Это один из главных итоговых outcomes всей реализации.

## Что агент должен делать прямо сейчас

Если агент подключился сегодня, ему не надо перечитывать всё подряд, чтобы понять общий статус.

Он должен:
1. Прочитать этот файл.
2. Увидеть:
   - что baseline уже есть;
   - что Stage 1-10 уже закрыты;
   - что следующий implementation overlay задаётся через `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md`;
   - что следующий честный queue-first action = `Queue 0`, если baseline не зафиксирован в текущем изменении, иначе `Queue 1`.
3. После этого открывать:
   - `docs/AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` как execution program;
   - `docs/IMPLEMENTATION_GATES.md` как proof/gate contract;
   - `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md` как canonical placement contour.

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

Простое правило:
- если этот файл не обновлён, агент снова будет вынужден “вычислять статус проекта” по косвенным признакам;
- это считается ошибкой внедрения, а не нормой.
