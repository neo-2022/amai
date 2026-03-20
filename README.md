modified_at: 2026-03-20 14:45 MSK
Ручная сверка guide/docs: 2026-03-20 14:45 MSK

# Art-memory-agent-index (Amai)

![Amai lockup](brand/amai_lockup.svg)

Amai — это отдельный внешний инструмент для ИИ-агентов.
Он помогает агентам работать сразу с несколькими репозиториями и при этом не путать их между собой.

Проще говоря, Amai делает четыре вещи:
- запоминает, с каким проектом сейчас работает агент;
- индексирует код и документы так, чтобы их можно было быстро находить;
- собирает для агента готовую подборку полезного контекста по запросу;
- не даёт по умолчанию смешивать данные разных проектов.

Это не плагин для одной IDE.
Это самостоятельный backend/tooling проект, который можно использовать из VS Code, Cursor, JetBrains, CLI или другого agent runtime.

## Что означают ключевые термины

- `проект`
  - отдельный репозиторий или рабочий корень, который агент должен видеть как самостоятельную сущность;
- `рабочая область проекта` (`namespace`)
  - именованная зона внутри проекта для правил поиска и доступа;
- `поиск контекста` (`retrieval`)
  - поиск нужных документов, символов, фрагментов кода и связанных материалов;
- `поиск по смыслу` (`semantic search`)
  - поиск не по точному совпадению слов, а по похожему смыслу;
- `готовая подборка контекста` (`context pack`)
  - собранный пакет найденных материалов с указанием происхождения каждого фрагмента;
- `provenance`
  - явное указание, из какого проекта, файла и места в коде пришёл фрагмент.

Клиентами могут быть:
- VS Code;
- Cursor;
- JetBrains IDE;
- CLI-агенты;
- CI;
- web UI;
- локальные orchestrators.

## Стек

- `PostgreSQL`
- `Qdrant`
- `S3-compatible object storage`
- `NATS Core + JetStream`
- `tree-sitter`
- `SQLite edge cache`
- `LanceDB` только optional на edge
- `Milvus` только как future scale-up replacement path

## Parser Baseline

Текущий code-structure слой materialize-ится не через агрегирующий pack, а через прямые Rust grammar crates поверх `tree-sitter`.

Сейчас реальный AST/symbol path покрывает:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Для остальных расширений проект пока делает честный `lexical-only fallback`, не ломая весь ingest.

## Карта Файлов Текущего Уровня

- `AGENTS.md`
  - обязательный вход для любого нового ИИ.
- `README.md`
  - краткий вход и карта проекта.
- `Cargo.toml`
  - Rust package, зависимости и бинарь `amai`.
- `compose.yaml`
  - локальный runnable stack.
- `.env.example`
  - обязательный конфигурационный шаблон.

## Карта Поддоменов

- `brand/`
  - канонический branding contour проекта: lockup, mark, favicon и brand spec.
- `docs/`
  - подробная архитектура, схема данных, операции и lifecycle.
- `config/`
  - конфиги сервисов и compatibility profile.
- `sql/`
  - каноническая схема PostgreSQL и seed-данные.
- `scripts/`
  - bootstrap, status и helper wrappers.
- `fixtures/`
  - нейтральные маленькие проекты для hardening и recovery proof.
- `src/`
  - Rust CLI и runtime bootstrap/index logic.
- `tests/`
  - локальные smoke и unit checks.
- `state/`
  - локальные данные контейнеров, не трекаются в git.
- `tmp/`
  - временные runtime-артефакты.

## Branding

Brand-pack проекта теперь хранится прямо в repo:
- [brand/README.md](/home/art/agent-memory-index/brand/README.md)
- [brand/amai_lockup.svg](/home/art/agent-memory-index/brand/amai_lockup.svg)
- [brand/amai_mark.svg](/home/art/agent-memory-index/brand/amai_mark.svg)
- [brand/favicon.ico](/home/art/agent-memory-index/brand/favicon.ico)
- [brand/amai_brand_spec.md](/home/art/agent-memory-index/brand/amai_brand_spec.md)

Правило использования:
- `README` и docs используют lockup;
- favicon и compact icon используют square mark или `favicon.ico`.

## Быстрый старт

1. Скопировать `.env.example` в `.env`
2. Запустить локальный стек:

```bash
cd /home/art/agent-memory-index
./scripts/bootstrap_stack.sh
```

3. Проверить, что всё поднялось:

```bash
./scripts/status.sh
```

Дополнительно можно прогнать:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_hostile.sh
```

4. Зарегистрировать свои проекты:

```bash
cargo run -- project register --code project_alpha --display-name "Project Alpha" --repo-root /path/to/project-alpha
cargo run -- project register --code project_beta --display-name "Project Beta" --repo-root /path/to/project-beta
cargo run -- relation add --source project_alpha --target project_beta --relation-type shared_runtime --shared-contour common_contour --access-mode local_plus_related
```

Или после первого `cargo build`:

```bash
./target/debug/amai project register --code project_alpha --display-name "Project Alpha" --repo-root /path/to/project-alpha
```

## Retrieval law

Правильный retrieval order:
1. определить активный проект;
2. понять, можно ли смотреть только его или ещё связанные проекты;
3. найти точные совпадения в Postgres;
4. найти подходящие символы через tree-sitter;
5. найти похожие по смыслу куски через Qdrant;
6. собрать готовую подборку контекста с источником каждого фрагмента.

Важно:
- unsupported parser language не должен валить индексирующий проход целиком;
- сначала сохраняется lexical/provenance baseline, затем по мере появления grammar coverage расширяется AST contour.

## Context Pack

`Amai` теперь materialize-ит не только indexing, но и agent-facing retrieval/context-pack contour:

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "how configuration is loaded" \
  --retrieval-mode local_strict
```

Команда:
- делает exact lookup по documents;
- делает symbol lookup;
- делает lexical chunk lookup;
- делает semantic chunk recall через Qdrant;
- собирает provenance-rich context pack;
- пишет его в PostgreSQL, SQLite edge cache и S3 context bucket.

## Verification contour

В проекте теперь есть отдельный verification layer, а не только smoke-скрипты.

Прямые Rust-команды:

```bash
cargo run -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --warmup 1 \
  --iterations 5 \
  --persist

cargo run -- verify hostile --scenario all
```

Что они доказывают:
- `verify benchmark`
  - мерит живой `context pack` path по времени;
  - выдаёт `mean/p50/p95/max`;
  - может fail-ить при нарушении заданных latency thresholds;
- `verify hostile`
  - проверяет fail-closed реакцию на partial-service loss;
  - проверяет recovery после возврата сервиса;
  - отдельно проверяет drift в `stack_meta`.

## Защита от version drift

В `Amai` есть отдельный compatibility contour:
- machine-readable профиль: [compatibility.toml](/home/art/agent-memory-index/config/compatibility.toml)
- live проверка:

```bash
cargo run -- compat check
```

Инструмент fail-closed ловит несовместимый drift по:
- `PostgreSQL`
- `Qdrant`
- `NATS`
- `stack_meta` schema/profile state

Для S3-compatible слоя сейчас удерживается API-совместимость и family-check без жёсткой блокировки по vendor string.

## Быстрый индексирующий smoke

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 5 \
  --skip-embeddings
```
