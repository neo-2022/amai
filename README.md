modified_at: 2026-03-20 14:08 MSK
Ручная сверка guide/docs: 2026-03-20 14:08 MSK

# Art-memory-agent-index (Amai)

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

- `docs/`
  - подробная архитектура, схема данных, операции и lifecycle.
- `config/`
  - конфиги сервисов, сейчас прежде всего NATS.
- `sql/`
  - каноническая схема PostgreSQL и seed-данные.
- `scripts/`
  - bootstrap, status и helper wrappers.
- `src/`
  - Rust CLI и runtime bootstrap/index logic.
- `tests/`
  - локальные smoke и unit checks.
- `state/`
  - локальные данные контейнеров, не трекаются в git.
- `tmp/`
  - временные runtime-артефакты.

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

## Быстрый индексирующий smoke

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 5 \
  --skip-embeddings
```
