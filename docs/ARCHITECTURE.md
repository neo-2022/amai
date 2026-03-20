modified_at: 2026-03-20 19:14 MSK
Ручная сверка guide/docs: 2026-03-20 19:14 MSK

# Architecture

## Цель

Сделать внешний стек для ИИ-агентов, который:
- по умолчанию держит проекты раздельно;
- умеет при необходимости читать связанный проект по явным правилам;
- быстро работает локально;
- остаётся бесплатным и self-hosted;
- спокойно расширяется примерно до `10` проектов и дальше без переделки основы.

Проще говоря, это не «ещё один индексатор кода».
Это общий служебный слой для агентов, который отвечает за память, поиск, правила доступа и сбор готового контекста.

## Главные роли слоёв

### PostgreSQL

Source of truth для:
- projects;
- namespaces;
- project_relations;
- retrieval policies;
- agent identities;
- runs/sessions metadata;
- memory cards metadata;
- code document metadata;
- exact lexical lookup.

### Qdrant

Semantic accelerator:
- code chunk vectors;
- memory card vectors;
- поиск похожих по смыслу фрагментов только после того, как правила уже определили допустимую область поиска.

### S3-compatible storage

Artifact plane:
- transcripts;
- snapshots;
- context bundles;
- raw evidence files.

Человечески:
- это слой, куда складываются тяжёлые артефакты;
- он не решает, что агенту можно видеть;
- он только хранит уже разрешённые результаты.

### NATS Core + JetStream

Event/work plane:
- indexing tasks;
- planner events;
- retries;
- fan-out notifications.

Не source of truth для state/policy.

### tree-sitter

Code structure plane:
- symbol extraction;
- module/function/type boundaries;
- symbol-aware chunking.

Человечески:
- этот слой понимает форму кода, а не только текст;
- поэтому агент может искать не только по словам, но и по функциям, типам и модулям.

### SQLite edge cache

Локальный cache агента:
- exact results;
- recent context packs;
- local FTS retrieval;
- cached document slices для быстрых повторных сессий.

### LanceDB

Не ядро.
Только optional local semantic edge cache при offline/local-first режиме.

## Retrieval modes

- `local_strict`
- `local_plus_related`
- `explicit_foreign`
- `audit_global`

В понятных словах:
- `local_strict`
  - смотреть только текущий проект;
- `local_plus_related`
  - смотреть текущий проект и явно связанные с ним;
- `explicit_foreign`
  - смотреть чужой проект только по прямой команде или политике;
- `audit_global`
  - редкий режим общего аудита.

## Provenance minimum

Каждый fragment обязан хранить:
- `source_project`
- `repo_root`
- `commit_sha`
- `path`
- `symbol`
- `chunk_id`
- `source_kind`
- `trust_level`

## Compatibility contour

Чтобы стек не ломался молча при частичном обновлении компонентов, в `Amai` есть отдельный compatibility contour.

Он делает три вещи:
- хранит machine-readable профиль совместимости;
- проверяет live версии ключевых сервисов;
- сверяет профиль в коде с profile/schema записью в PostgreSQL.

Сейчас compatibility профиль удерживает:
- schema version;
- поддерживаемый major для `PostgreSQL`;
- поддерживаемый major/minor для `Qdrant`;
- поддерживаемый major/minor для `NATS`;
- S3-family check без жёсткой блокировки по vendor string.

Принцип fail-closed:
- если стек вышел за поддерживаемый профиль, `Amai` не должен молча продолжать indexing/context-pack operations;
- сначала оператор должен увидеть drift и либо вернуть совместимую версию, либо осознанно обновить compatibility manifest.

## Materialized baseline

Текущий baseline этого проекта materialize-ится как Rust-first CLI `amai`:
- `bootstrap stack`
  - применяет PostgreSQL schema;
  - создаёт app-role и grants;
  - создаёт Qdrant collections и aliases;
  - создаёт S3 buckets;
  - создаёт NATS streams;
  - создаёт SQLite edge cache.
- `project register`
  - регистрирует project root и default namespace.
- `namespace ensure`
  - создаёт namespace внутри уже зарегистрированного проекта.
- `relation add`
  - создаёт controlled cross-project relation edge.
- `index project`
  - индексирует файлы в PostgreSQL;
  - извлекает symbols/chunks через прямые Rust grammar crates поверх `tree-sitter`;
  - пишет code chunk vectors в Qdrant;
  - пишет exact cache в SQLite.
- `context pack`
  - определяет effective retrieval mode;
  - строит visible project set через relation graph;
  - для каждого видимого проекта дополнительно разрешает только тот же `namespace` code, который запросил агент;
  - делает exact document lookup в PostgreSQL;
  - делает symbol lookup в PostgreSQL;
  - делает lexical chunk lookup в PostgreSQL;
  - сначала делает semantic chunk recall в Qdrant;
  - если vector tier временно возвращает пустой результат на локальном tiny contour, использует уже найденные lexical chunks как explicit semantic fallback, не скрывая provenance;
  - если lexical/symbol/exact evidence вообще нет, а semantic hits не перекрывают query terms по path/content, делает semantic abstention вместо слабого шума;
  - materialize-ит provenance-rich context pack в PostgreSQL, SQLite edge cache и S3 context bucket.
- `mcp serve`
  - materialize-ит stdio MCP server поверх уже существующего retrieval/observability baseline;
  - отдаёт tools для `list projects`, `list namespaces`, `context pack`, `token benchmark`, `observe snapshot`, `warm cache`;
  - отдаёт prompts, которые сразу объясняют новому ИИ, что `Amai` делает и почему по умолчанию нужен `local_strict`.
- `mcp config`
  - генерирует client-specific config snippets без ручного копирования внутренних runtime настроек в IDE.

Текущий parser baseline:
- полноценный AST/symbol contour для `rust`, `toml`, `javascript`, `typescript`, `tsx`, `json`;
- честный lexical fallback для остальных файлов до добавления отдельных grammar crates.

## Verification plane

Отдельно от runtime planes у проекта теперь materialized verification plane.

Его задача:
- не только проверять, что стек поднимается;
- но и доказывать, что он:
  - fail-closed ведёт себя при partial-service loss;
  - восстанавливается после возврата сервиса;
  - держит practical latency baseline для живого `context pack` path.

Текущие verification contours:
- `scripts/proof_local.sh`
  - быстрый formatting + test + compat + status proof;
- `scripts/proof_hardening.sh`
  - repeat bootstrap, relation-aware retrieval и restart recovery;
- `scripts/proof_performance.sh`
  - end-to-end latency proof для `context pack` с hot guard `<10ms p95`;
- `scripts/proof_accuracy.sh`
  - relation-aware precision и zero-leakage isolation proof;
- `scripts/proof_load.sh`
  - concurrent hot-load proof для reproducible QPS/error-rate baseline c guard `qps >= 5000` и `p95 < 10ms`;
- `scripts/proof_token_benchmark.sh`
  - measured token-economy proof для naivе scope vs compact context-pack render;
- `scripts/proof_token_benchmark_suite.sh`
  - multi-query token-economy proof, который мерит уже не один удачный prompt, а серию запросов и агрегирует `mean/p50/p95`;
- `scripts/proof_mcp.sh`
  - end-to-end MCP handshake/tool/prompt proof на живом fixture stack;
- `scripts/proof_hostile.sh`
  - hostile proof на `stack_meta` drift и service loss для `postgres`, `qdrant`, `minio`, `nats`;
- `cargo run -- verify benchmark ...`
  - Rust-native latency verifier с threshold enforcement;
- `cargo run -- verify accuracy ...`
  - Rust-native verifier для `cross_project_leakage`, `symbol_precision`, `semantic_precision`;
- `cargo run -- verify load ...`
  - Rust-native concurrent load verifier для `qps/error_rate/p95`;
- `cargo run -- verify token-benchmark ...`
  - Rust-native measured token-economy verifier;
- `cargo run -- verify token-benchmark-suite ...`
  - Rust-native verifier для набора запросов с честным агрегированием продуктовой экономии токенов;
- `cargo run -- verify mcp ...`
  - Rust-native verifier, который сам проходит MCP handshake, tools, prompts и tool calls;
- `cargo run -- verify hostile ...`
  - Rust-native hostile verifier с fail-closed and recovery proof.

## Observability plane

Поверх verification plane теперь materialized и отдельный observability plane.

Его задача:
- не только сказать, что сервисы поднялись;
- а снять воспроизводимый snapshot состояния стека;
- сравнить его с machine-readable SLA профилем;
- сохранить этот snapshot в PostgreSQL для следующих сравнений.

Канонические команды:
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`
- `cargo run -- observe serve --bind 0.0.0.0:9464`

Machine-readable профиль:
- [observability.toml](/home/art/agent-memory-index/config/observability.toml)

Слои observability:
- `PostgreSQL`
  - connection saturation
  - probe latency
  - transaction counters
  - deadlocks
  - WAL bytes
- `Qdrant`
  - vectors total
  - optimization queue
  - update queue
  - resident memory
  - semantic search stage p95 через last cold retrieval benchmark
- `NATS`
  - publish probe latency
  - consumer lag
  - JetStream disk usage
- `Indexing`
  - last index throughput
 - `Accuracy`
   - cross-project leakage
   - symbol precision
   - semantic precision
- `Load`
  - hot qps
  - hot error rate
- `Token economy`
  - naive visible-scope tokens
  - context-pack tokens
  - saved tokens
  - savings factor / savings percent
  - parser coverage ratio
- `Retrieval`
  - `hot benchmark`
  - `cold benchmark`

Принцип честности:
- `hot` и `cold` retrieval не смешиваются;
- `hot` нужен для реальной скорости повторной сессии агента;
- `cold` нужен для оценки настоящего retrieval path без result-cache shortcut;
- быстрый hot-path измеряется в микросекундах и сохраняется как дробные миллисекунды, чтобы убрать ложный `0ms` эффект после агрессивной локальной оптимизации;
- SLA нельзя честно считать выполненным, если известен только один из этих режимов;
- scrape path Prometheus exporter обязан быть read-only;
- exporter не должен писать `system_snapshot` в PostgreSQL на каждый scrape, иначе monitoring сам начнёт искажать state и latency baseline.

## Monitoring profile

Поверх observability plane materialized и monitoring profile:
- Prometheus rules;
- Grafana dashboard;
- встроенный Rust exporter `/metrics`.

Роли:
- `observe snapshot`
  - снимает и сохраняет canonical snapshot в PostgreSQL;
- `observe sla-check`
  - снимает snapshot и fail-ит при `critical`/`unknown`;
- `observe serve`
  - только публикует Prometheus metrics;
  - не становится source of truth и не подменяет explicit snapshots.

Таким образом monitoring разделён на два слоя:
- stateful evidence layer;
- read-only scrape layer.
