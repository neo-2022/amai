modified_at: 2026-03-20 14:08 MSK
Ручная сверка guide/docs: 2026-03-20 14:08 MSK

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
  - делает exact document lookup в PostgreSQL;
  - делает symbol lookup в PostgreSQL;
  - делает lexical chunk lookup в PostgreSQL;
  - делает semantic chunk recall в Qdrant;
  - materialize-ит provenance-rich context pack в PostgreSQL, SQLite edge cache и S3 context bucket.

Текущий parser baseline:
- полноценный AST/symbol contour для `rust`, `toml`, `javascript`, `typescript`, `tsx`, `json`;
- честный lexical fallback для остальных файлов до добавления отдельных grammar crates.
