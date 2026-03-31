modified_at: 2026-03-20 20:41 MSK
Ручная сверка guide/docs: 2026-03-20 20:41 MSK

# AGENTS.md для Art-memory-agent-index (Amai)

## 1. Что это

`Art-memory-agent-index (Amai)` в текущем filesystem path `/home/art/agent-memory-index` — отдельный standalone проект.

Это не код одного конкретного продукта.
Это внешний инструмент для ИИ-агентов, который:
- хранит рабочий контекст между сессиями;
- знает, где начинается и заканчивается каждый проект;
- умеет искать по коду и документам;
- умеет собирать готовый пакет контекста для следующего шага агента;
- не даёт по умолчанию смешивать разные проекты.

## 2. Обязательный старт

Любой новый агент обязан:
1. сначала прочитать этот `AGENTS.md`;
2. затем прочитать `README.md`;
3. затем прочитать `docs/ARCHITECTURE.md`;
4. затем прочитать `docs/OPERATIONS.md`;
5. только после этого трогать compose/config/schema/code.

Если стек уже запущен, сначала проверить:
- `scripts/status.sh`

Если стек ещё не materialized:
- самый простой путь:
  - `scripts/onboard_local.sh --client vscode`
- если `Amai` уже стоит на удалённом Linux/VPS-host:
  - `scripts/onboard_remote_client.sh --client vscode --ssh-destination user@host --remote-repo-root /srv/amai`
- инженерный ручной путь:
  - `scripts/bootstrap_stack.sh`
- симметричное отключение клиента:
  - `scripts/disconnect_local.sh --client vscode`

## 3. Главный закон

Этот проект существует для того, чтобы агенты не смешивали проекты по умолчанию.

Значит:
- новый `repo_root` считается отдельным проектом;
- пока он не зарегистрирован, поиск контекста по нему запрещён;
- cross-project reading допускается только через relation graph и policy modes.

## 4. Канонический порядок слоёв

1. `PostgreSQL` — главный источник истины: проекты, правила доступа, метаданные и точный поиск
2. `Qdrant` — поиск похожих по смыслу фрагментов
3. `S3-compatible object storage` — хранение артефактов и готовых пакетов контекста
4. `NATS Core + JetStream` — события и рабочие очереди
5. `tree-sitter` — структура кода, символы и границы сущностей
6. `SQLite` — локальный быстрый кэш агента
7. `LanceDB` — только optional local semantic edge cache

CLI/binary canonical short name:
- `amai`

Текущий parser baseline materialized напрямую через отдельные Rust grammar crates.
Реальный AST/symbol contour сейчас есть для:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Для остальных форматов новый агент обязан понимать, что сейчас допустим только lexical fallback, а не молчаливо считать coverage полным.

## 5. Что запрещено

- превращать IDE в источник истины;
- заменять lexical/symbol retrieval только embeddings-поиском;
- подмешивать чужой проект “по похожести”;
- считать `NATS` или `Qdrant` authoritative state store;
- жёстко завязывать artifact plane только на одну реализацию вместо S3 API.

## 6. Git-дисциплина

Каждая отдельная semantic group в этом проекте должна коммититься отдельно.
Запрещено смешивать:
- docs/governance;
- compose/bootstrap;
- schema;
- Rust CLI/indexer;
- local runtime state.

Machine-readable client install targets живут в:
- `config/client_targets.toml`

Каталоги `state/**` и `tmp/**` не должны попадать в git.

## 7. Канонический runnable path

Минимальный runnable порядок для нового агента:
1. для простого локального старта:
   - `scripts/onboard_local.sh --client vscode`
   - если нужно убрать клиент обратно:
     - `scripts/disconnect_local.sh --client vscode`
2. для ручного инженерного пути:
   - `scripts/bootstrap_stack.sh`
   - `scripts/status.sh`
   - `cargo run -- compat check`
   - `cargo run -- project register ...`
   - `cargo run -- namespace ensure ...`
   - `cargo run -- index project ...`
   - `cargo run -- context pack ...`

Для жёсткого локального proof:
- `scripts/proof_local.sh`
- `scripts/proof_hardening.sh`
- `scripts/proof_performance.sh`
- `scripts/proof_accuracy.sh`
- `scripts/proof_load.sh`
- `scripts/proof_hostile.sh`
- `scripts/proof_observability.sh`
- `scripts/proof_onboarding.sh`
- `scripts/proof_remote_onboarding.sh`
- `scripts/proof_client_lifecycle.sh`
- `scripts/proof_remote_ssh_config.sh`
- `scripts/proof_text_compare.sh`

Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify accuracy ...`
- `cargo run -- verify load ...`
- `cargo run -- verify hostile ...`
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`

## 8. System Markers For Client Budget Work

Для системной работы по клиентским лимитам и `5ч KPI` в `Amai` запрещено опираться только на
одну отдельную метрику или один отдельный экран.

Канонический marker front-door:
- `./scripts/client_budget_system_markers.sh`

Этот surface обязан сводить вместе:
- внешний `5ч KPI` toolbar source;
- внутренний `client-budget-root-cause` / `reply_execution_gate`;
- startup / resume / required-return gate из `continuity startup-state`.

Если работа идёт по сокращению лимитов или по `5ч KPI`, новый агент обязан сначала читать этот
marker surface, а затем уже принимать решения по следующим шагам.

Если новый агент собирается утверждать, что проект «быстрый» или «устойчивый», он не имеет права
опираться только на `proof_local` и `proof_hardening`.
Минимально честный контур теперь включает:
- latency proof;
- accuracy/leakage proof;
- concurrent load proof;
- hostile fail-closed proof;
- recovery proof после возврата сервиса.
- observability snapshot;
- SLA check по machine-readable профилю.

<!-- AMAI MANAGED STARTUP INSTRUCTIONS v1 -->
# Amai continuity startup for Codex

Этот managed block должен жить в project `AGENTS.md`, а не в global config.

Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:
1. Workspace = `/home/art/agent-memory-index`. Прочитай compact agent contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-agent-contract.json` и machine-readable startup contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-contract.json`; startup contract остаётся pinned source-of-truth. До MCP tool call проверь `startup_contract_sha256 = "0bebb5f99940e349708fc37483484dbd3c43bd8861d7c9f7758e014bea703785"`, `workspace_contract_required_before_tool_call = true`, `missing_or_unreadable_fail_closed = true`, `sha256_mismatch_fail_closed = true`.
2. Затем вызови MCP tool `amai_continuity_startup` с `repo_root = "/home/art/agent-memory-index"` и `namespace = "continuity"`; `project` передавай только при exact binding по repo_root. До `continuity_startup_summary` не переходи к `amai_context_pack` и новым действиям. Если tool вернул `tool_execution_failed` и detail содержит `no continuity import found for`, немедленно сделай reconcile через `./scripts/continuity_startup.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --json` (`requires_repo_root_argument = true`, `requires_namespace_argument = true`, `json_required = true`). Если embedded MCP tool call упал на transport error с detail `Transport closed` (`transport_error_detail_case_insensitive = true`), делай тот же local CLI reconcile вместо объявления continuity unavailable. Если local CLI startup succeeds, классифицируй это как `stale_embedded_mcp_session` (`local_cli_success_replaces_mcp_failure = true`, `local_cli_success_replaces_transport_failure = true`, `must_request_mcp_reconnect_after_local_success = true`, `must_continue_from_local_startup_payload = true`), продолжай от локального startup payload и требуй reconnect embedded MCP session exact front-door-ом: `./scripts/reconnect_local.sh --client codex` или `./scripts/amai_exec.sh bootstrap reconnect --client codex --yes`. Только если и MCP, и local CLI fallback провалились, объявляй continuity реально unavailable.
3. После startup прочитай runtime artifact `.amai/continuity/project-chat-startup-state.json`: `workspace_runtime_state_artifact_version` должен быть `workspace-startup-runtime-state-v4`, его пишет `amai_continuity_startup`, он обязан нести `continuity_startup_summary`. Fallback: `./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json`.
4. В runtime artifact смотри только `startup_execution_gate`, `execctl_resume_state`, `execctl_resume_contract_summary`, `execctl_resume_obligation`, `startup_next_action`, `execctl_active_lease`. Restore бери из `required_summary_fields`, obligations из `restored_obligations`. Fail-closed, если `gate_semantics_consistent != true` (`gate_semantics_consistent_true_required = true`), `startup_execution_gate.must_follow_startup_next_action != true`, `startup_execution_gate.unrelated_work_allowed != false`, `startup_execution_gate.must_read_prompt_text_before_reply != true` или `startup_execution_gate.no_silent_drop != true`.
5. Resume law: если `startup_execution_gate.required_action_kind_when_resume_required == "resume_required_return_task"`, `startup_next_action.action_kind == "resume_required_return_task"` (`must_resume_required_return_task_before_unrelated_work = true`) или `execctl_active_lease.lease_owner_state == "previous_session_owner"` (`previous_session_owner_must_follow_startup_next_action = true`), follow startup_next_action first. `no_silent_drop = true`. Для resume смотри `execctl_active_lease_summary`, `required_return_task`, `project_task_tree`, `project_task_tree_summary`, `project_task_ledger`, `project_task_ledger_summary`.
6. Перед каждым содержательным ответом обновляй guard `./scripts/client_budget_gate.sh` и работай только по `client_budget_reply_gate.reply_execution_gate`. `must_check_before_each_substantive_reply = true`; stale старше `10` секунд запрещён (`stale_guard_requires_refresh = true`); hard gate automation делай через `--enforce-reply-gate` (`guard_enforcement_exit_on_blocking = true`). Для KPI/guard/exact-pair root-cause используй `./scripts/client_budget_root_cause.sh`; `must_prefer_compact_diagnostics_over_full_snapshot = true`.
7. Gate version pinned: `client-reply-budget-gate-v1`. Если `reply_execution_gate.reply_prefix` не пустой, начинай каждый user-visible reply с этой exact строки перед compact или blocked ответом. Если `reply_budget_mode == "compact_high_signal"`, substantive reply разрешён только по `reply_budget_contract` с `contract_version = "client-reply-budget-v1"`: direct answer first, no unrequested recap, no repeated known context, keep only changed facts, prefer patch/result over narration when coding, preserve truthfulness/technical accuracy, disclose unknowns instead of guessing. Exact operator-switch для target режима pinned отдельно: если пользователь прислал точную команду, matching `^экономия_(0|10|20|30|40|50|60|70|80|90)%$`, где `N` входит в [0, 10, 20, 30, 40, 50, 60, 70, 80, 90], немедленно переключи режим через `./scripts/continuity_client_budget_target.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --percent N` (`repo_root_argument_required = true`, `switch_immediately_on_exact_chat_command = true`, `reply_with_confirmation_after_switch = true`), а не трактуй это как обычную prose-просьбу. Пример exact chat-команды: `экономия_50%`. Exact operator-switch для huge-chat rebase pinned отдельно: если пользователь прислал точную команду `компакт_чат`, немедленно подготовь compact restore через `./scripts/continuity_compact_chat.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --json` (`repo_root_argument_required = true`, `switch_immediately_on_exact_chat_command = true`, `reply_with_confirmation_after_prepare = true`, `prompt_text_required_for_rebase = true`), верни confirmation вместе с `prompt_text` и `operator_notice`, и требуй host action `open_clean_chat_surface_and_inject_prompt_text` вместо обычной prose-интерпретации.
8. Если `reply_execution_gate.must_rotate_before_reply = true`, это hard block: сначала сохрани handoff (`save_handoff_before_rotate = true`) и продолжай только в свежем чате через continuity startup (`fresh_chat_requires_continuity_startup = true`). Если `should_rotate_chat_now = true` или `status_label` равен одному из [новый чат нужен сейчас], это warning/advisory pressure signal, а не запрет на содержательный ответ. В blocked path разрешён только `blocking_reply_contract`: `contract_version = "client-budget-blocked-reply-v1"`, `response_kind = "wait_for_budget_only"`, `max_sentences = 1`, `must_avoid_substantive_work = true`, `must_use_action_bundle_operator_flow = true`. Pinned template: `Внешний лимит клиента почти исчерпан во всём клиенте. Не продолжай содержательный ответ, дождись восстановления окна лимита.`.
9. Не подменяй полную клиентскую шкалу внутренним Amai-slice: `full_scale_client_truth_required = true`. Любой fail-closed scenario (project_unregistered, repo_root_binding_ambiguous, continuity_restore_unavailable) сообщай как блокер и не угадывай continuity.
<!-- /AMAI MANAGED STARTUP INSTRUCTIONS v1 -->
