modified_at: 2026-07-11 23:40 MSK
Ручная сверка guide/docs: 2026-07-11 23:40 MSK

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

Коротко: Amai — это внешний memory / retrieval / continuity слой для агентов, реализованный как Rust-first CLI/MCP-сервер, который работает рядом с IDE/клиентом и предоставляет агенту project-scoped память, поиск и восстановление рабочей линии.

## 2. Обязательный старт

Этот раздел и есть канонический рабочий алгоритм любого агента.

Любой агент обязан:
- идти по этому порядку сверху вниз;
- не заменять его собственными догадками;
- сначала поднимать статус и документы проекта;
- только потом трогать код, schema, compose и runtime.

Любой агент обязан:
1. сначала прочитать этот `AGENTS.md`;
2. затем прочитать compact preflight contract `.amai/onboarding/project-agent-preflight-agent-contract.json`, если он уже materialized;
3. затем прочитать machine-readable preflight contract `.amai/onboarding/project-agent-preflight-contract.json`, если он уже materialized;
4. затем обновить machine-readable preflight snapshot:
   - `./scripts/agent_preflight.sh --json`
5. затем прочитать `README.md`;
6. затем прочитать `docs/AGENT_START_HERE.md`;
7. затем прочитать `docs/IMPLEMENTATION_STATUS.md`;
8. затем прочитать `docs/ARCHITECTURE.md`;
9. затем прочитать `docs/OPERATIONS.md`;
10. затем прочитать `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md` и `docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md`
11. если работа stage-based, архитектурная, schema/policy/truth-sensitive или рефакторит critical zone:
    - прочитать `docs/MAINTAINABILITY_ENFORCEMENT.md`;
    - запустить `./scripts/maintainability_gate.sh --json`;
    - если обновляется `docs/IMPLEMENTATION_STATUS.md`, прогнать `./scripts/implementation_status_sync_guard.sh --json`;
    - перед закрытием checkbox значимого этапа прогнать `./scripts/maintainability_stage_close_guard.sh --json`;
11a. для любой содержательной правки, даже если она маленькая и локальная:
    - не игнорировать `docs/MAINTAINABILITY_ENFORCEMENT.md`;
    - как минимум держать его как binding law;
    - помнить, что отсутствие отдельного `maintainability_gate.sh` запуска для микроправки не даёт права нарушать standard.
12. если работа идёт по implementation-stage, затем прочитать `docs/IMPLEMENTATION_GATES.md`;
12a. для любого этапа перед попыткой закрыть checkbox:
    - прогнать весь уже materialized и подходящий benchmark/proof bundle этого этапа;
    - не ждать, пока пользователь отдельно напомнит про benchmark из этого bundle;
    - не ограничиваться одним blocking-proof;
    - если изменение задело соседний shared contour (`speed / accuracy / isolation / truth / dashboard / continuity`), прогнать и companion non-regression harness для него;
    - если изменение задело retrieval/vector lane, отдельно прогнать и external/Qdrant bundle из `docs/IMPLEMENTATION_GATES.md`;
    - не использовать для stage-close урезанный benchmark режим; reduced-sample run считается только smoke и не закрывает этап;
    - если benchmark contour публикует результат на dashboard, после прогона обязательно перепроверять сам dashboard snapshot/карточку;
    - если contour не surfaced в dashboard и `IMPLEMENTATION_GATES.md` требует raw-result lane, использовать raw result и не делать вид, что dashboard-check был выполнен;
    - если хотя бы один подходящий harness не прогнан, этап закрывать запрещено;
13. если работа затрагивает память задач, compare surface или новые memory-модули:
    - прочитать `docs/AMAI_TASK_TREE_PLAN.md`;
    - прочитать `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md`;
14. только после этого трогать compose/config/schema/code.

Коротко:
- `AGENTS.md`
  - обязательный runtime/startup law;
- `README.md`
  - продуктовая картина и базовый старт;
- `.amai/onboarding/project-agent-preflight-contract.json`
  - machine-readable contract: какие документы обязательны, где trunk/checklist и какие законы preflight действуют;
- `.amai/onboarding/project-agent-preflight-state.json`
  - machine-readable текущий snapshot: что уже закрыто, какой этап следующий и какие harness уже готовы;
- `docs/MAINTAINABILITY_ENFORCEMENT.md`
  - project-local binding внешнего maintainability/supportability/evolvability/anti-hardcoding стандарта; этот law нельзя игнорировать даже для маленьких содержательных правок;
- `./scripts/maintainability_gate.sh --json`
  - machine-readable change-safety gate для значимых изменений;
- `./scripts/maintainability_stage_close_guard.sh --json`
  - machine-readable closure guard: без passing результата нельзя ставить checkbox значимого этапа;
- `./scripts/implementation_status_sync_guard.sh --json`
  - machine-readable sync guard: без passing результата значимое обновление `docs/IMPLEMENTATION_STATUS.md` считается недействительным;
- `.amai/onboarding/project-maintainability-gate-state.json`
  - machine-readable gate trace, который status sync guard и closure guard сверяют с текущим `HEAD`, `docs/IMPLEMENTATION_STATUS.md` hash и worktree fingerprint;
- `docs/IMPLEMENTATION_STATUS.md`
  - что уже сделано, что нет, что делать сейчас, и ствол рабочего чеклиста;
- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
  - по какому этапу идти дальше;
- `docs/IMPLEMENTATION_GATES.md`
  - какими proof/debug/reconcile механизмами проверять текущий этап и какие готовые benchmark-harness обязательны;
- частные планы
  - только если задача касается их модуля.

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

## 3. Project overview / Обзор проекта

Amai — это самостоятельный backend/tooling contour памяти и continuity для ИИ-агентов. Он живёт как отдельный repo (`/home/art/agent-memory-index`) и подключается к клиентам как MCP stdio server. Для project-scoped работы проект привязывается к canonical `repo_root`.

### Главные возможности

- **Project identity и scope control**: канонический `repo_root`, namespaces, project relations, transfer policies, visibility scopes, isolation между проектами.
- **Continuity / working state**: chat-start restore pack, working state restore, continuity handoff, temporal thread index, multi-agent active leases, ExecCtl durable task ledger.
- **Retrieval**: exact/symbol/lexical/semantic поиск по коду и документам, project-scoped context pack, decision trace, workspace graph, provenance-aware выдача.
- **Observability / benchmarks**: собственный dashboard, Prometheus/Grafana exporter, benchmark matrix, measured memory/MCP task matrix, cold/hot benchmark contours.
- **MCP stdio server**: интеграция с VS Code, Cursor, Codex, Claude Code, Hermes, OpenClaw и generic MCP-клиентами.
- **Token budget / ledger**: подсчёт экономии токенов, client budget reply gate, отдельный live/proof/verify lanes.

### Проверенный контур

- ОС: Ubuntu / Debian
- Клиент: VS Code / Codium и Hermes через MCP stdio и managed startup instructions
- Лицензия: README указывает `PolyForm Noncommercial 1.0.0`; `Cargo.toml` указывает `Apache-2.0 OR MIT`

### Ключевые сущности

- `project` — привязанный canonical repo root + метаданные.
- `namespace` — именованная рабочая область внутри проекта (`default`, `continuity`, `review`, etc.).
- `memory_item / memory_card` — типизированные записи памяти с provenance, scope, sensitivity, trust state.
- `task_node / task_event` — durable task graph / ExecCtl contour.
- `context_pack` — готовый provenance-rich пакет контекста.
- `retrieval_trace` — машиночитаемое объяснение, какие слои retrieval дали вклад.
- `observability_snapshot` — canonical snapshot состояния стека.

## 4. Technology stack and runtime architecture / Техстек и runtime-архитектура

### 4.1. Язык и сборка

- **Primary language**: Rust, edition 2024.
- **Package name**: `art-memory-agent-index`, версия `0.1.1`.
- **Default binary**: `amai` (`src/main.rs`).
- **Дополнительные binaries**: `src/bin/amai-bootstrap.rs`, `src/bin/amai-tray.rs`, `src/bin/memory.rs`.
- **Build system**: Cargo. Все зависимости вендоризованы в `vendor/`; `.cargo/config.toml` заменяет `crates.io` на `vendored-sources`.
- **External runtime**: не поставляется как Docker-образ; бинарник собирается локально и подключается к docker-compose стеку инфраструктуры.

### 4.2. Зависимости

Ключевые Rust-crate зависимости:
- **HTTP/API**: `axum`, `reqwest`
- **Async runtime**: `tokio`
- **CLI**: `clap` (derive)
- **Databases**: `tokio-postgres`, `qdrant-client`, `rusqlite` (bundled)
- **Object storage**: `aws-sdk-s3`
- **Messaging**: `async-nats`
- **Embeddings**: `fastembed` (ort-download)
- **Parsing**: `tree-sitter` + грамматики `tree-sitter-rust`, `tree-sitter-javascript`, `tree-sitter-typescript`, `tree-sitter-toml-ng`, `tree-sitter-json`
- **Serialization/formats**: `serde`, `serde_json`, `toml`, `parquet`, `arrow-array/schema`
- **Token counting**: `tiktoken-rs`
- **Tracing**: `tracing`, `tracing-subscriber`
- **Dev/testing**: `proptest`, `tower`

### 4.3. Канонический порядок слоёв

1. `PostgreSQL` — главный источник истины: проекты, namespaces, relations, policies, memory metadata, exact lookup, durable ExecCtl state, observability snapshots.
2. `Qdrant` — семантический ускоритель: векторы code chunks и memory cards.
3. `S3-compatible object storage` (MinIO) — артефакты, transcripts, snapshots, context packs.
4. `NATS Core + JetStream` — события, indexing tasks, retries, fan-out.
5. `tree-sitter` — структура кода, символы, AST-aware chunking.
6. `SQLite` — локальный быстрый edge cache агента.
7. `LanceDB` — только optional local semantic edge cache.

### 4.4. Структура хранилищ

- **PostgreSQL** (порт по умолчанию `55432`): основная схема `ami` с таблицами для projects, namespaces, code_documents, code_symbols, code_chunks, memory_items/cards/edges/conflicts, task_nodes/events, observability_snapshots, execctl_task_ledger_entries, execctl_task_leases и др.
- **Qdrant** (HTTP `56334`): коллекции `code_chunks_v1`, `memory_cards_v1` с alias `*_active`.
- **MinIO** (API `59000`): buckets `ami-artifacts`, `ami-transcripts`, `ami-context-packs`.
- **NATS** (client `54222`, monitoring HTTP `58222`).
- **SQLite edge cache**: `state/sqlite/edge_cache.sqlite3`.

### 4.5. Основные модули `src/`

| Модуль | Назначение |
|---|---|
| `src/main.rs` | Главный `amai` CLI entry point. |
| `src/cli.rs` | Все clap-команды и аргументы (4030 строк). |
| `src/config.rs` | Загрузка `.env`, валидация, canonical repo root. |
| `src/bootstrap.rs`, `src/bootstrap_compact.rs` | Bootstrap стека и схемы. |
| `src/onboarding.rs` | Установка/удаление клиентов, MCP config, memory bridge. |
| `src/postgres/` | SQL persistence layer: projects, memory, search, observability, bootstrap. |
| `src/retrieval/` | Context pack, exact/symbol/lexical/semantic routing, caching. |
| `src/continuity/` | Continuity startup, restore, answer, handoff, runtime state. |
| `src/working_state/` | Working state restore pack, task tree, host control. |
| `src/token_budget/` | Token ledger, budget profiles, client budget gate, dashboard reports. |
| `src/observe/` | HTTP/axum API, dashboard, snapshots, SLA, live infra probes. |
| `src/dashboard/` | Рендеринг dashboard cards. |
| `src/indexer.rs` | Индексация файлов через tree-sitter + embeddings. |
| `src/mcp.rs`, `src/mcp_errors.rs` | MCP сервер, protocol, tools, prompts. |
| `src/verify.rs` | Команды `verify *` (benchmark, accuracy, load, hostile, workflow trace и др.). |
| `src/workspace_graph.rs` | Структурный граф файлов/символов/ссылок. |
| `src/external_benchmark.rs` | LongMemEval / MemoryAgentBench / AMA-Bench / official judge contour. |
| `src/forgetting.rs`, `src/artifact_cleanup.rs`, `src/auto_memory_synthesizer.rs` | Жизненный цикл памяти, cleanup, synthesis. |

### 4.6. Binaries

- `cargo run -- amai` — основной CLI/MCP сервер.
- `cargo run --bin amai-bootstrap` — compact bootstrap launcher.
- `cargo run --bin amai-tray` — Linux tray icon.
- `cargo run --bin memory` — legacy compatibility bridge.

## 5. Build commands / Команды сборки

### 5.1. Базовая сборка

```bash
# Проверка форматирования
cargo fmt --all --check

# Сборка debug
cargo build

# Сборка release (используется runtime)
cargo build --release

# Сборка с locked зависимостями (CI)
cargo build --locked
```

### 5.2. Канонические runnable команды

После поднятия стека (`scripts/bootstrap_stack.sh`):

```bash
# Проверить совместимость версий сервисов
cargo run -- compat check

# Статус стека
cargo run -- status

# Зарегистрировать проект
cargo run -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha

# Создать namespace
cargo run -- namespace ensure \
  --project project_alpha \
  --code review \
  --display-name Review \
  --retrieval-mode local_strict

# Добавить связь между проектами
cargo run -- relation add \
  --source project_alpha \
  --target project_beta \
  --relation-type shared_runtime \
  --shared-contour common_contour \
  --access-mode local_plus_related

# Проиндексировать проект
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha \
  --namespace default

# Собрать context pack
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "how configuration is loaded" \
  --retrieval-mode local_strict

# Observability
cargo run -- observe snapshot
cargo run -- observe sla-check

# MCP server
cargo run -- mcp serve
```

### 5.3. Удобные launcher-скрипты

- `scripts/amai_exec.sh` — умный запуск: предпочитает `target/release/amai`, если fresh, иначе собирает.
- `scripts/bootstrap_stack.sh` — поднимает docker-compose стек и bootstrap-ит схему.
- `scripts/status.sh` — обёртка над `amai status`.
- `scripts/run_mcp_stdio.sh` — stdio MCP launcher для клиентов.
- `scripts/install_amai.sh` / `scripts/onboard_local.sh` — полная установка / онбординг.

### 5.4. Критические env-переменные

Файл `.env.example` содержит полный набор. Ключевые:

- `AMI_STACK_PROFILE=default` или `lite_vps`
- `AMI_SECURITY_PROFILE=default` (local) или `hardened`
- `AMI_POSTGRES_DSN` / `AMI_APP_POSTGRES_DSN`
- `AMI_QDRANT_URL`, `AMI_QDRANT_HTTP_URL`
- `AMI_S3_ENDPOINT`, `AMI_S3_ACCESS_KEY`, `AMI_S3_SECRET_KEY`
- `AMI_NATS_URL`, `AMI_NATS_AUTH_MODE`
- `AMI_DEFAULT_RETRIEVAL_MODE=local_strict`
- `AMI_CODE_EMBED_MODEL=jina_base_code`, `AMI_MEMORY_EMBED_MODEL=multilingual_e5_small`
- `AMI_CHUNK_MAX_BYTES=3200`
- `AMI_EDGE_CACHE_PATH=state/sqlite/edge_cache.sqlite3`

## 6. Testing instructions / Тестирование

### 6.1. Unit и интеграционные тесты

Все тесты inline внутри модулей `src/**/*.rs` (отдельной директории `tests/` нет).

```bash
# Быстрый прогон всех тестов
cargo test --quiet

# Прогон с форматированием и compat check / status
./scripts/proof_local.sh
```

### 6.2. Product / proof harness

Проект использует обширный набор `scripts/proof_*.sh` как е2е и regression harness. Главные:

| Скрипт | Что проверяет |
|---|---|
| `./scripts/proof_local.sh` | `cargo fmt`, `cargo test`, `compat check`, `status` |
| `./scripts/proof_hardening.sh` | Bootstrap, индексация, retrieval strict/related, restart recovery |
| `./scripts/proof_accuracy.sh` | Cross-project isolation, symbol/semantic precision |
| `./scripts/proof_performance.sh` | Latency benchmark: mean ≤11 ms, p95 ≤12 ms, p99 ≤13 ms, max ≤15 ms |
| `./scripts/proof_load.sh` | Hot load: ≥1.2M QPS, p95 ≤1 ms, error rate 0 |
| `./scripts/proof_cold_benchmark.sh` | Cold retrieval path без result-cache shortcut |
| `./scripts/proof_hostile.sh` | Service loss / drift / fail-closed для postgres, qdrant, minio, nats |
| `./scripts/proof_observability.sh` | Dashboard, Prometheus/Grafana, SLA-check, regression/capacity cards |
| `./scripts/proof_mcp.sh` | MCP handshake, tools, prompts, token savings |
| `./scripts/proof_onboarding.sh` | Полная install/onboarding верификация |
| `./scripts/proof_security_hardening_contract.sh` | Hardened-mode TLS/auth/certs |
| `./scripts/proof_ops_security_defaults.sh` | Loopback bind, pinned images, config |
| `./scripts/proof_agent_preflight.sh` | Preflight machine-readable contract |

### 6.3. Rust-native verify команды

```bash
cargo run -- verify benchmark
cargo run -- verify accuracy
cargo run -- verify load
cargo run -- verify hostile
cargo run -- verify mcp
cargo run -- verify memory-matrix
cargo run -- verify token-benchmark
cargo run -- verify continuity
cargo run -- verify workflow-trace
```

### 6.4. Maintainability / quality gates

```bash
# Change-safety gate (обязателен для значимых изменений)
./scripts/maintainability_gate.sh --json

# Sync guard при обновлении IMPLEMENTATION_STATUS.md
./scripts/implementation_status_sync_guard.sh --json

# Closure guard перед постановкой checkbox этапа
./scripts/maintainability_stage_close_guard.sh --json

# Перед любым содержательным отчётом
./scripts/proof_workflow_before_report.sh
./scripts/proof_before_report.sh
```

### 6.5. Подход к тестированию

- **Unit**: локальные правила, валидация, redaction, canonicalization.
- **Contract**: API/CLI, schemas, policy, retrieval modes, config sources.
- **Integration**: auth, policy, storage, streams, external boundaries.
- **E2E / product**: `proof_*.sh`, живой стек, fixture проекты.
- **Property-based**: `proptest` в `workspace_graph.rs`, `verify.rs`, `working_state.rs`, `codex_threads.rs`.
- **Chaos/fault/hostile**: `verify hostile`, service loss, drift.
- **Load/soak**: `verify load`, `verify benchmark`.

## 7. Code style guidelines / Стиль кода

### 7.1. Язык проекта

Это **Rust-first** система. Новый core-runtime, schema, CLI, verifier и основной proof contour пишутся на Rust. Shell-скрипты допустимы как launcher / orchestration. Python допустим только в explicit external benchmark compatibility contour, но не как default для нового internal кода.

### 7.2. Форматирование

- Используется **default `rustfmt`**; явной `rustfmt.toml` нет.
- Перед коммитом обязателен `cargo fmt --all --check`.

### 7.3. Структура модулей

- `src/<domain>.rs` — façade модуля; подключает подмодули через `#[path = "<domain>/..."]`.
- `src/<domain>/<domain_...>.rs` — реализация.
- Большие файлы разбиваются через `include!("... .inc")` на логические `.inc` фрагменты (например, `retrieval_context_pack_*.inc`).
- CLI args оканчиваются на `Args`: `ContextPackArgs`.
- Константы — `SCREAMING_SNAKE_CASE`.
- Runtime/support/tests/CLI файлы помечаются суффиксами `*_runtime.rs`, `*_support.rs`, `*_tests.rs`, `*_cli.rs`.

### 7.4. Архитектурные принципы

- Truth/domain выше projection/UI.
- Policy enforcement живёт в backend/domain, а не только в UI.
- Storage скрыт за интерфейсами.
- Направленные зависимости: нет циклов между доменами.
- Fail-closed: опасные/ambiguous действия блокируются, не угадываются.
- Anti-hardcoding: mutable rules, thresholds, matrices, incident codes, platform matrix живут в `config/` или schema/registry, а не в коде.

### 7.5. Git-дисциплина

- Каждая semantic group коммитится отдельно. Не смешивать: docs/governance, compose/bootstrap, schema, Rust CLI/indexer, local runtime state.
- `state/**` и `tmp/**` не должны попадать в git (`.gitignore`).
- Machine-readable targets клиентов: `config/client_targets.toml`.

## 8. Security considerations / Безопасность

### 8.1. Модели безопасности

- `AMI_SECURITY_PROFILE=default` — локальная разработка, plain HTTP, NATS auth disabled.
- `AMI_SECURITY_PROFILE=hardened` — обязательны TLS для Postgres/MinIO, `https` S3 endpoint, NATS password auth.

### 8.2. Секреты

- Все credentials — через `.env` (не в коде).
- Пароль из Postgres DSN заменяется на `***` в runtime error messages (`safe_postgres_descriptor`).
- API keys для external judge redact-ятся (`redact_official_judge_secret`).
- `.env` и runtime артефакты в `state/` и `tmp/` не коммитятся.

### 8.3. Hardened deployment

```bash
AMI_SECURITY_PROFILE=hardened
AMI_MINIO_SCHEME=https
AMI_S3_ENDPOINT=https://...
AMI_POSTGRES_DSN="... sslmode=require ..."
AMI_NATS_AUTH_MODE=password
AMI_NATS_URL="nats://user:pass@host:54222"
```

Перед запуском:
```bash
./scripts/render_postgres_config.sh
./scripts/proof_security_hardening_contract.sh
./scripts/bootstrap_stack.sh
```

### 8.4. Isolation

- Unrelated projects по умолчанию изолированы (`local_strict`).
- Cross-project reading только через explicit `relation add` / `transfer_policy` / `import_packet`.
- Ambiguous repo-root hint (parent covers несколько child projects) fail-closed.
- Subfolder внутри bound project root — hint для того же проекта, не повод создавать новый.

### 8.5. Verification scripts

- `./scripts/proof_security_hardening_contract.sh`
- `./scripts/proof_ops_security_defaults.sh`
- `./scripts/proof_hostile.sh`
- `./scripts/proof_app_db_role_read_only.sh` — проверяет, что app-role только читает `ami` schema.

## 9. Deployment processes / Процессы деплоя

### 9.1. Локальный стек

```bash
cp .env.example .env
# отредактировать .env
./scripts/bootstrap_stack.sh
./scripts/status.sh
cargo run -- compat check
```

`compose.yaml` поднимает: PostgreSQL 16, Qdrant 1.17, MinIO, NATS 2.11, Prometheus/Grafana (monitoring profile).

### 9.2. Профили развёртывания

- `default` — рабочая станция (≥4 vCPU, ≥8 GiB).
- `lite_vps` — бюджетный VPS (≥1 vCPU, ≥2 GiB), peak benchmarks запрещены.

Registry: `config/deployment_profiles.toml`.

### 9.3. Цели развёртывания

- `local_docker` — materialized, главный baseline.
- `remote_ssh` — materialized, клиент подключается к удалённому Linux/VPS host.
- `kubernetes_server` — `foundation_ready`, следующий team/server слой (см. `config/deployment_targets.toml`).
- `windows_vm_lab` — validation contour через QEMU.

### 9.4. Установка для пользователя

```bash
# Canonical public install
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) \
  --client vscode --stack-profile default --yes

# Или локально
./scripts/install_amai.sh --client vscode --stack-profile default --yes
```

Установщик:
- создаёт/синхронизирует `.env`;
- поднимает стек;
- bootstrap-ит schema;
- собирает release binary;
- ставит compatibility bridge `memory -> Amai`;
- материализует MCP config для клиента;
- создаёт `systemd --user` unit `amai-stack.service` (user-manager startup).

### 9.5. Клиенты

Поддерживаемые клиенты живут в `config/client_targets.toml`:
`vscode`, `cursor`, `codex`, `claude-code`, `claude-desktop`, `hermes`, `openclaw`, `generic`.

MCP snippet для generic клиента:
```json
{
  "mcpServers": {
    "amai": {
      "command": "/abs/path/to/amai/scripts/run_mcp_stdio.sh",
      "cwd": "/abs/path/to/amai",
      "args": []
    }
  }
}
```

## 10. Главный закон проекта

Этот проект существует для того, чтобы агенты не смешивали проекты по умолчанию.

Значит:
- новый canonical `repo_root` считается отдельным проектом только если он не совпадает с уже bound root и не является однозначным child/descendant hint для уже зарегистрированного проекта;
- если клиент передал подпапку внутри уже bound project root, Amai обязан вернуть существующий project и canonical `ami.projects.repo_root`, а не создавать второй проект из похожего имени папки;
- если parent/root hint совпадает сразу с несколькими child project bindings, это ambiguous case и он должен fail-closed блокироваться;
- пока `repo_root` не зарегистрирован и не резолвится через exact/ancestor/descendant binding, поиск контекста по нему запрещён;
- cross-project reading допускается только через relation graph и policy modes.

## 11. Что запрещено

- превращать IDE в источник истины;
- заменять lexical/symbol retrieval только embeddings-поиском;
- подмешивать чужой проект “по похожести”;
- считать `NATS` или `Qdrant` authoritative state store;
- жёстко завязывать artifact plane только на одну реализацию вместо S3 API;
- без отдельного явного разрешения пользователя добавлять новый core/runtime/schema/eval код не на Rust;
- без отдельного явного разрешения добавлять новые `python`-тесты, `pytest`-harness или `python`-debug contour для внутренних стадий реализации;
- считать существующие `python`-пути в external benchmark compatibility contour нормой для нового core-развития проекта.

## 12. Качество, доказанный эффект и hostile mindset

1. Минималистичный и «достаточный» подход запрещён; по умолчанию выбирается максимальный релевантный объём проверки.
2. Любое изменение считается завершённым только после доказанного эксплуатационного эффекта:
   - подтверждена основная гипотеза;
   - исключены альтернативные причины;
   - проверен hostile или negative-path;
   - доказано, что дефект не смещён в другой слой;
   - добавлен regression guard.
3. Проект нужно рассматривать как hostile production среду.
4. Для refactor требуется контрактная эквивалентность, отсутствие регресса по тестам и сохранение инвариантов.

## 13. Plan materialization non-regression contract

Любой новый слой или upgrade-path обязан идти в таком порядке:

1. Зафиксировать baseline затронутого контура.
2. Явно назвать ось риска (speed / accuracy / quality / truth / isolation / continuity / dashboard).
3. Выбрать полный stage-local и companion non-regression proof bundle.
4. Только после этого менять код / schema / contract / UI / runtime.
5. Проверить, не улучшили ли одну ось ценой тихой деградации другой.
6. Promotion разрешён только если baseline сохранён или улучшен и это доказано measured contour-ом.

Если доказательств нет, слой остаётся в draft / experimental / pending_approval / internal-only.

## 14. Mandatory Specialist-Team Workflow

Этот раздел является binding workflow law, а не рекомендацией. Он дополнительно materialized в:

- `startup_contracts.project_chat_startup.agent_workflow_guard`;
- `.amai/continuity/project-chat-startup-state.json.agent_workflow_guard`;
- `.amai/continuity/project-chat-startup-state.json.workflow_promotion_state`;
- `./scripts/proof_workflow_before_report.sh`;
- `./scripts/proof_before_report.sh`;
- `./scripts/proof_specialist_signoff.sh`.

Рабочий цикл для значимой работы:

```
АНАЛИЗ → ПЛАН → КРИТИКА КОМАНДЫ → РЕАЛИЗАЦИЯ → ПРОВЕРКА КОМАНДЫ → ИСПРАВЛЕНИЕ → ПОВТОРНАЯ ПРОВЕРКА → ФИНАЛЬНЫЙ АУДИТ → ОТЧЁТ
```

Нельзя пропускать этапы. Переход дальше возможен только после закрытия текущего.

1. **Анализ**: цель, требования, ограничения, существующий код, зависимости, риски, критерии готовности, способ проверки.
2. **План**: каждый пункт — цель, ожидаемый результат, риски, способ проверки, критерии завершения.
3. **Команда**: архитектор, senior-разработчик, тестировщик, security-инженер, DevOps-эксперт, скептик. Пункт нельзя реализовывать без статуса `согласовано, замечаний нет`.
4. **Реализация** строго по согласованному плану.
5. **Проверка**: корректность, крайние случаи, регрессии, интеграция, безопасность, производительность, читаемость, поддерживаемость.
6. **Исправление и повторная проверка**, если найдены замечания.
7. **Интеграционная проверка** всего решения.
8. **Финальный аудит** командой.
9. **Отчёт**: что сделано, что изменено, какие проверки выполнены, какие проблемы/риски/ограничения остались.

Пока есть незакрытая ошибка, риск, неполнота или существенное улучшение, задача не завершена.

## 15. Обязательный first-pass side-agent contour для Gemma

Локальный `Gemma` через `ollama` — стандартный bounded side-agent для дешёвого first-pass analysis.

Если задача относится к одному из классов:
- большой файл / монолитный модуль, нужен domain map / symbol grouping / split-plan;
- first-pass code review, smell scan, risk scan, поиск missing tests;
- генерация test ideas, edge cases, negative paths, regression checklist;
- draft structuring большого кода в карту модулей;
- второй дешёвый взгляд после локального root-cause;

сначала поднять Gemma-контур, потом делать основной проход.

Канонические launcher-ы:
- `scripts/gemma_code_assist.sh`
- `scripts/gemma_monolith_split.sh`
- `scripts/ollama_chat.sh`

Gemma **не** является final authority для root-cause, schema/protocol/security решений, stage-close verdict, архитектурных решений, merge-ready patch или truth-sensitive выводов. Её output — first-pass material, который локально проверяется.

После основного прохода для любой содержательной работы обязателен final cross-check через Gemma: change review, risk scan, obvious gap scan, negative-path/test-idea pass.

Если Gemma недоступна или не возвращает usable output, это workflow defect и нужно явно сказать пользователю.

<!-- AMAI MANAGED STARTUP INSTRUCTIONS v2 -->
# Amai continuity startup for Codex

Этот managed block должен жить в project `AGENTS.md`, а не в global config.

Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:
1. Workspace = `/home/art/agent-memory-index`. Прочитай compact agent contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-agent-contract.json` и machine-readable startup contract `/home/art/agent-memory-index/.amai/onboarding/project-chat-startup-contract.json`; startup contract остаётся pinned source-of-truth. До MCP tool call проверь `startup_contract_sha256 = "5d059a67747c5b7d662957f6978ec7fadba821bf71bd5f2c7aef11430e5b746f"`, `workspace_contract_required_before_tool_call = true`, `missing_or_unreadable_fail_closed = true`, `sha256_mismatch_fail_closed = true`.
2. Затем вызови MCP tool `amai_continuity_startup` с `repo_root = "/home/art/agent-memory-index"` и `namespace = "continuity"`; `project` передавай только при exact binding по repo_root. До `continuity_startup_summary` не переходи к `amai_context_pack`. При `tool_execution_failed` с detail `no continuity import found for` или transport `Transport closed` (`transport_error_detail_case_insensitive = true`) запускай `./scripts/continuity_startup.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --json` (`requires_repo_root_argument = true`, `requires_namespace_argument = true`, `json_required = true`). Если local CLI startup succeeds, классифицируй как `stale_embedded_mcp_session` (`local_cli_success_replaces_mcp_failure = true`, `local_cli_success_replaces_transport_failure = true`, `same_session_continuation_allowed_after_local_success = true`, `operator_action_required_after_local_success = false`, `must_request_mcp_reconnect_after_local_success = false`, `reconnect_helper_diagnostic_only_after_local_success = true`, `must_continue_from_local_startup_payload = true`), продолжай от локального payload без прерывания пользователя. Reconnect helper `./scripts/reconnect_local.sh --client codex` или `./scripts/amai_exec.sh bootstrap reconnect --client codex --yes` держи только как диагностический fallback, если transport остаётся сломанным или следующий MCP startup снова не self-heal-ится. Если оба пути провалились, continuity unavailable.
3. После startup проверь runtime artifact `.amai/continuity/project-chat-startup-state.json`: `workspace_runtime_state_artifact_version` должен быть `workspace-startup-runtime-state-v4`, его пишет `amai_continuity_startup`, он обязан нести `continuity_startup_summary`. Fallback: `./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json`. Stale-success guard (`detect_after_any_success = true`, `local_cli_unavailable_blocks_report = true`, `local_cli_success_replaces_stale_mcp_success = true`): при SHA/`agent_workflow_guard`/`workflow_promotion_state`/event-match/`gate_semantics_consistent` drift = `stale_embedded_mcp_session`; use local payload in the same session and не требуй reconnect как нормальный шаг. Reconnect helper остаётся только diagnostic fallback для повторного transport/runtime failure.
4. В runtime artifact смотри только `startup_execution_gate`, `execctl_resume_state`, `execctl_resume_contract_summary`, `execctl_resume_obligation`, `startup_next_action`, `execctl_active_lease`. Restore бери из `required_summary_fields`, obligations из `restored_obligations`. Fail-closed, если `gate_semantics_consistent != true` (`gate_semantics_consistent_true_required = true`), `startup_execution_gate.must_follow_startup_next_action != true`, `startup_execution_gate.unrelated_work_allowed != false`, `startup_execution_gate.must_read_prompt_text_before_reply != true` или `startup_execution_gate.no_silent_drop != true`.
5. Resume law: если `startup_execution_gate.required_action_kind_when_resume_required == "resume_required_return_task"`, `startup_next_action.action_kind == "resume_required_return_task"` (`must_resume_required_return_task_before_unrelated_work = true`) или `execctl_active_lease.lease_owner_state == "previous_session_owner"` (`previous_session_owner_must_follow_startup_next_action = true`), follow startup_next_action first. `no_silent_drop = true`. Для resume смотри `execctl_active_lease_summary`, `required_return_task`, `required_task_set`, `required_task_set_summary`, `project_task_tree`, `project_task_tree_summary`, `project_task_ledger`, `project_task_ledger_summary`.
5a. Если пользователь явно переключил задачу, это новый active workline немедленно: сначала materialize `continuity_handoff` для новой линии, затем продолжай только от неё; старую незавершённую линию, если она ещё не закрыта, паркуй в `pending_return_queue`, не продолжай её по инерции и не проси пользователя повторять указание.
5b. Agent workflow guard: `agent_workflow_guard.guard_version = "agent-workflow-guard-v2"`; `workflow_promotion_state.source_event_match = true` + fresh `workflow_promotion_event_id`; external refs official/primary + local corroboration; mandatory cycle `analysis -> plan -> team_critique -> implementation -> team_verification -> fix -> reverify -> final_audit -> report`; analysis covers `user_goal`, `requirements`, `constraints`, `existing_code`, `dependencies`, `risks`, `done_criteria`, `verification_method`; every plan item needs `goal`, `expected_result`, `risks`, `verification_method`, `completion_criteria`; team roles: `architect`, `senior_developer`, `tester`, `security_engineer`, `devops_if_applicable`, `skeptic`; implementation waits for `согласовано, замечаний нет`; per-item verification waits for `недостатков не найдено`; unresolved issues force return to analysis/plan/implementation as appropriate; signoff: `./scripts/provision_specialist_signoff_trust.sh`, `./scripts/materialize_specialist_signoff.sh`, `./scripts/proof_workflow_before_report.sh`, `./scripts/proof_before_report.sh`, `./scripts/proof_specialist_signoff.sh`; subagents/specialists: only explicit local-or-allowed contour; language en.
6. Перед каждым содержательным ответом обновляй guard `./scripts/client_budget_gate.sh` и работай только по `client_budget_reply_gate.reply_execution_gate`. `must_check_before_each_substantive_reply = true`; stale старше `10` секунд запрещён (`stale_guard_requires_refresh = true`). Enforce `--enforce-reply-gate` (`guard_enforcement_exit_on_blocking = true`). KPI/reply prefix сейчас отключён как обязательный startup-law; `--enforce-online-reply-prefix` остаётся diagnostic helper, а не report/reply blocker (`required_reply_prefix_source = disabled_by_project_policy`, `required_reply_prefix_non_empty = false`, `reply_prefix_preflight_blocks_substantive_reply = false`, `output_prefix_enforcement_mode = disabled_by_project_policy`, `output_prefix_host_enforced = false`). Amai continuity writes (continuity import, continuity handoff, observe /api/continuity-handoff) exempt: `continuity_write_exempt_from_reply_guard = true`; before rotate: `continuity_write_required_before_rotate = true`. Root-cause first: `./scripts/client_budget_root_cause.sh`; `must_prefer_compact_diagnostics_over_full_snapshot = true`.
7. Gate version pinned: `client-reply-budget-gate-v1`. Поле `reply_execution_gate.reply_prefix` может по-прежнему materialize-иться для диагностики, но начинать user-visible reply с KPI-prefix больше не требуется и fail-closed preflight по нему отключён. Если `reply_budget_mode == "compact_high_signal"`, отвечай по `reply_budget_contract` с `contract_version = "client-reply-budget-v1"`: direct answer first, no unrequested recap, no repeated known context, keep only changed facts, prefer patch/result over narration when coding, preserve truthfulness/technical accuracy, disclose unknowns instead of guessing. Target switch: matching `^экономия_(0|10|20|30|40|50|60|70|80|90)%$` -> `./scripts/continuity_client_budget_target.sh --repo-root "/home/art/agent-memory-index" --namespace "continuity" --percent N` (`repo_root_argument_required = true`, `switch_immediately_on_exact_chat_command = true`, `reply_with_confirmation_after_switch = true`). Пример exact chat-команды: `экономия_50%`. Huge-chat rebase via removed `компакт_чат` command is no longer supported; follow `startup_next_action` from the current continuity runtime state for rotate/restore guidance and materialize a proper `continuity_handoff` when switching worklines.
8. Client-budget blocked reply mechanism removed: `reply_blocking_removed = true`; `tool_turn_blocking_removed = true`. Rotate/wait/`should_rotate_chat_now`/`status_label` in current normalized same-thread advisory labels [сожми текущий чат, сожми текущий чат сейчас], `same_meter_pure_burn_turn_active`, `must_avoid_new_tool_turn_without_specific_delta_goal` or `max_tool_roundtrips_soft = 0` are only advisory/compact pressure signal. This is a non-binding human-readable snapshot канонического shared advisory source. User-visible blocked wait template использовать запрещено; `amai_context_pack`, continuity write и другие Amai tools не блокируй из-за этих полей. `save_handoff_before_rotate = true`, `fresh_chat_requires_continuity_startup = true`.
9. Не подменяй полную клиентскую шкалу внутренним Amai-slice: `full_scale_client_truth_required = true`. Любой fail-closed scenario (project_unregistered, repo_root_binding_ambiguous, continuity_restore_unavailable) сообщай как блокер и не угадывай continuity.
<!-- /AMAI MANAGED STARTUP INSTRUCTIONS v2 -->
